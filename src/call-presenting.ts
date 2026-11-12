import { computed, ref, watch, type Ref } from "vue";
import { toastController } from "@ionic/vue";
import { documentOutline, easelOutline } from "ionicons/icons";
import type { Router } from "vue-router";
import { closeOnBackWhile } from "./back";
import {
  PREMIUM_PAGE,
  acceptFile,
  chat,
  fileBelongsTo,
  grantPlugin,
  loadMessages,
  needsSubscription,
  pickFiles,
  readMessageFile,
  sendPicked,
} from "./core";
import { installed, isLocked, pluginIcon, pluginImage, pluginName, refreshPlugins, refreshPremiumLock, type HandedFile } from "./plugins";
import {
  PRESENT_BOARD,
  PRESENT_DOCUMENT,
  PRESENT_FILE_LIMIT,
  SEND_WAIT,
  canPresentWith,
  installOffered,
  needsPresentGrant,
  presentGrant,
  sentFile,
  waitFor,
} from "./present";
import { call, cannotPresent, presentInCall, stopPresenting } from "./calls";
import { t } from "./i18n";

export interface CallPresentingOptions {
  id: Readonly<Ref<string>>;
  contact: Readonly<Ref<{ name: string }>>;
  current: Readonly<Ref<boolean>>;
  onScreen: Readonly<Ref<boolean>>;
  router: Router;
  sheet: Readonly<Ref<{ close: () => Promise<void> } | null>>;
}

interface Asking {
  them: boolean;
  name: string;
  body: string;
  icon?: string;
  image?: string;
  allow: () => Promise<void>;
}

type PresentState = "none" | "missing" | "ask" | "declined" | "accept" | "loading" | "failed" | "tooBig" | "sheet";

// kept after its presentation until it says goodbye, so both phones hear ft.onClose
interface Shown {
  key: string;
  plugin: { id: string; name: string };
  file?: HandedFile;
  lead: boolean;
}

export function useCallPresenting({ id, contact, current, onScreen, router, sheet }: CallPresentingOptions) {
  const presenting = computed(() => (current.value && call.phase === "active" ? call.presenting : null));
  // a lost link resends the same presentation, keep the tool while the key holds
  const presentKey = computed(() => (presenting.value ? `${presenting.value.by}|${presenting.value.plugin}|${presenting.value.file ?? ""}` : ""));
  // only the board and the PDF viewer present, anything else opens nothing
  const presentPlugin = computed(() => {
    const id = presenting.value?.plugin;
    return id === PRESENT_BOARD || id === PRESENT_DOCUMENT ? installed.value.find((one) => one.id === id) : undefined;
  });
  const canOffer = computed(() => current.value && call.native && call.canPresent && call.phase === "active" && !call.presenting);
  const choosing = ref(false);
  const starting = ref(false);
  async function once(work: () => Promise<unknown>) {
    if (starting.value) return;
    starting.value = true;
    try {
      await work();
    } finally {
      starting.value = false;
    }
  }

  const asking = ref<Asking | null>(null);
  const declined = ref("");

  async function say(key: string) {
    const toast = await toastController.create({ message: t(key), duration: 4000, position: "top" });
    await toast.present();
  }

  const presentButtons = computed(() => [
    { text: t("calls.presentBoard"), icon: easelOutline, handler: () => void present(PRESENT_BOARD) },
    { text: t("calls.presentDocument"), icon: documentOutline, handler: () => void present(PRESENT_DOCUMENT) },
    { text: t("common.cancel"), role: "cancel" },
  ]);

  async function grantLive(tool: string): Promise<boolean> {
    const plugin = installed.value.find((one) => one.id === tool);
    if (plugin) await grantPlugin(plugin.id, presentGrant(plugin)).catch(() => undefined);
    if (!(await refreshPlugins().then(() => true, () => false))) return false;
    const now = installed.value.find((one) => one.id === tool);
    return Boolean(now && !needsPresentGrant(now));
  }

  function present(tool: string) {
    choosing.value = false;
    // both pressed at once, theirs shows
    if (call.presenting) return;
    return once(async () => {
      await Promise.all([refreshPlugins(), refreshPremiumLock()]);
      const added = await installOffered(tool);
      // installing takes seconds, theirs may have started meanwhile
      if (call.presenting) return;
      if (added === "failed") return say("calls.presentFailed");
      const plugin = installed.value.find((one) => one.id === tool);
      if (!canPresentWith(plugin)) return say("calls.presentMissing");
      if (isLocked(plugin)) return void router.push(PREMIUM_PAGE);
      if (!needsPresentGrant(plugin)) return begin(tool);
      asking.value = {
        them: false,
        name: pluginName(plugin),
        body: t("plugins.live"),
        icon: pluginIcon(plugin),
        image: pluginImage(plugin),
        allow: () =>
          once(async () => {
            if (call.presenting) return;
            if (await grantLive(tool)) await begin(tool);
            else await say("calls.presentFailed");
          }),
      };
    });
  }

  async function begin(tool: string) {
    try {
      if (tool !== PRESENT_DOCUMENT) return await presentInCall(call.id, tool);
      const [file] = await pickFiles("application/pdf");
      // the picker can outlive the call or the start of theirs
      if (!file || call.phase !== "active" || call.presenting) return;
      if (file.size > PRESENT_FILE_LIMIT) return await say("calls.presentTooBig");
      await loadMessages(id.value);
      const before = new Set((chat(id.value)?.messages ?? []).map((one) => one.id));
      await sendPicked(id.value, file);
      const message = await waitFor(() => sentFile(chat(id.value)?.messages ?? [], before, file.size), SEND_WAIT);
      if (!message) throw new Error("the PDF did not show up in the chat");
      await presentInCall(call.id, PRESENT_DOCUMENT, message);
    } catch (error) {
      // the plan may have changed since the check, the core has the last word
      if (needsSubscription(error)) return void router.push(PREMIUM_PAGE);
      if (call.presenting?.by === "them") return;
      await say(cannotPresent(error) ? "calls.cannotPresentOld" : "calls.presentFailed");
    }
  }

  async function stop() {
    try {
      await stopPresenting(call.id);
    } catch {
      await say("calls.presentFailed");
    }
  }

  function answerAsk(yes: boolean) {
    const ask = asking.value;
    asking.value = null;
    if (!ask) return;
    if (yes) void ask.allow();
    else if (ask.them) declined.value = presentKey.value;
  }

  function closeOwnSheets() {
    choosing.value = false;
    if (asking.value && !asking.value.them) asking.value = null;
  }

  const fileMessage = computed(() => {
    const file = presenting.value?.file;
    return file ? chat(id.value)?.messages.find((one) => one.id === file) : undefined;
  });
  const presentFile = ref<HandedFile | undefined>();
  const presentBroken = ref(false);
  // opened cold from CallKit the list is still empty, so not "missing" yet
  const pluginsRead = ref(false);
  async function readPlugins() {
    await refreshPlugins().catch(() => undefined);
    pluginsRead.value = true;
  }
  const seeding = ref("");
  const seedFailed = ref("");
  let toFollow = "";
  function follow() {
    const tool = presenting.value?.plugin;
    if (!toFollow || !onScreen.value || !tool) return;
    const key = toFollow;
    toFollow = "";
    void followWith(key, tool);
  }
  watch(onScreen, follow);
  async function followWith(key: string, tool: string) {
    seeding.value = key;
    try {
      await readPlugins();
      if ((await installOffered(tool)) === "failed") seedFailed.value = key;
    } finally {
      if (seeding.value === key) seeding.value = "";
    }
  }
  const presentState = computed<PresentState>(() => {
    const now = presenting.value;
    if (!now) return "none";
    const plugin = presentPlugin.value;
    if (!canPresentWith(plugin)) {
      if (seedFailed.value === presentKey.value) return "failed";
      return pluginsRead.value && seeding.value !== presentKey.value ? "missing" : "loading";
    }
    if (presentBroken.value) return "failed";
    if (needsPresentGrant(plugin)) return declined.value === presentKey.value ? "declined" : "ask";
    if (!now.file || presentFile.value) return "sheet";
    if (now.by === "me") return "loading";
    const arrived = fileMessage.value?.file;
    if (arrived?.state === "failed") return "failed";
    if ((arrived?.bytes ?? 0) > PRESENT_FILE_LIMIT) return "tooBig";
    return arrived?.state === "waiting" ? "accept" : "loading";
  });
  const waitLabel = computed(() => (presenting.value?.by === "them" ? t("calls.presenting", { name: contact.value.name }) : t("calls.present")));
  const presentArea = computed(() => ["sheet", "accept", "loading", "failed", "tooBig"].includes(presentState.value));

  watch(
    () => Boolean(call.presenting),
    (now) => {
      if (now) closeOwnSheets();
    },
  );

  const granting = ref("");

  // Ionic moves an open sheet to the app root, so don't ask while another screen is in front
  watch(
    [presentState, asking, onScreen],
    ([state, , shown]) => {
      const plugin = presentPlugin.value;
      const key = presentKey.value;
      if (shown && state === "ask" && plugin && presenting.value?.by === "them" && !asking.value && granting.value !== key) {
        asking.value = {
          them: true,
          name: pluginName(plugin),
          body: t("calls.presentAsk", { name: contact.value.name, plugin: pluginName(plugin) }),
          icon: pluginIcon(plugin),
          image: pluginImage(plugin),
          allow: async () => {
            granting.value = key;
            try {
              if (!(await grantLive(plugin.id)) && presentKey.value === key) presentBroken.value = true;
            } finally {
              if (granting.value === key) granting.value = "";
            }
          },
        };
      }
      if (state !== "ask" && asking.value?.them) asking.value = null;
    },
    { immediate: true },
  );

  watch(
    presentKey,
    (key) => {
      presentFile.value = undefined;
      presentBroken.value = false;
      seedFailed.value = "";
      declined.value = "";
      toFollow = "";
      if (!key) return;
      const tool = presenting.value?.plugin;
      if (presenting.value?.by === "them" && (tool === PRESENT_BOARD || tool === PRESENT_DOCUMENT)) {
        toFollow = key;
        follow();
      } else void readPlugins();
      if (presenting.value?.file) void loadMessages(id.value).catch(() => undefined);
    },
    { immediate: true },
  );
  // their file's message may arrive after the presentation
  watch(
    [presentKey, presentState, () => (presenting.value?.by === "them" ? fileMessage.value?.file?.state : undefined)],
    async ([key, state, arrived]) => {
      const now = presenting.value;
      if (state !== "loading" || !now?.file || (now.by === "them" && arrived !== "done")) return;
      const ours = now.by === "me" || (await fileBelongsTo(now.file, id.value));
      const file = ours ? await readMessageFile(now.file).catch(() => undefined) : undefined;
      if (presentKey.value !== key) return;
      if (file) presentFile.value = file;
      else presentBroken.value = true;
    },
    { immediate: true },
  );

  // a new round mounts a fresh sheet instead of patching the closing one
  const onSheet = ref<(Shown & { round: number }) | null>(null);
  let sheetRounds = 0;
  const sheetLeaving = ref(false);
  const wanted = computed<Shown | null>(() => {
    const now = presenting.value;
    const plugin = presentPlugin.value;
    if (!onScreen.value || presentState.value !== "sheet" || !now || !plugin) return null;
    return { key: presentKey.value, plugin: { id: plugin.id, name: pluginName(plugin) }, file: presentFile.value, lead: now.by === "me" };
  });

  function showSheet() {
    if (sheetLeaving.value) return;
    const want = wanted.value;
    if (onSheet.value && onSheet.value.key !== want?.key) {
      sheetLeaving.value = true;
      if (sheet.value) void sheet.value.close();
      else sheetClosed();
    } else if (!onSheet.value && want) onSheet.value = { ...want, round: ++sheetRounds };
  }

  function sheetClosed() {
    sheetLeaving.value = false;
    onSheet.value = null;
    showSheet();
  }
  watch(() => wanted.value?.key ?? "", showSheet, { immediate: true });

  function acceptPresented() {
    const file = presenting.value?.file;
    if (file) void acceptFile(file).catch(() => undefined);
  }

  function presentRefused() {
    if (onSheet.value?.key === presentKey.value) presentBroken.value = true;
  }

  function presentDone() {
    if (presenting.value?.by === "me") void stop();
  }

  closeOnBackWhile(() => onScreen.value && choosing.value, () => (choosing.value = false));
  closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => answerAsk(false));

  return {
    presenting,
    presentPlugin,
    canOffer,
    choosing,
    starting,
    asking,
    declined,
    presentButtons,
    presentState,
    waitLabel,
    presentArea,
    onSheet,
    sheetLeaving,
    present,
    stop,
    answerAsk,
    closeOwnSheets,
    readPlugins,
    acceptPresented,
    presentRefused,
    presentDone,
    sheetClosed,
  };
}
