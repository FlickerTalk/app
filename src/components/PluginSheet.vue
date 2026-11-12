<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { isPlatform, toastController } from "@ionic/vue";
import {
  grantPlugin,
  pickForPlugin,
  takePhotoForPlugin,
  pluginMayUseDrive,
  pluginFetch,
  pluginForget,
  pluginChat,
  pluginLiveSend,
  pluginLocation,
  translateDetect,
  translateLanguages,
  translatePrepare,
  translateRemove,
  translateStatus,
  translateText,
  pluginMade,
  pluginOpen,
  pluginOpenChat,
  pluginPrint,
  pluginRead,
  pluginRecordForget,
  pluginRecordGet,
  pluginRecordKeys,
  pluginRecordSet,
  pluginRecordUsage,
  pluginSave,
  pluginWrite,
  remindCancel,
  remindList,
  remindSet,
  sendPicked,
  vaultBackup,
  vaultBackupInfo,
  vaultCancelPending,
  vaultConnect,
  vaultDisconnect,
  vaultDownload,
  vaultList,
  vaultMkdir,
  vaultMove,
  vaultOpen,
  vaultRemove,
  vaultRename,
  vaultRestore,
  vaultRetry,
  vaultSave,
  vaultStatus,
  vaultUploadMessage,
  vaultUploadPicked,
  PLUGIN_EVENT,
  type PickedFile,
  type PluginEvent,
  type PluginHookName,
  type Sending,
} from "../core";
import { i18n } from "../i18n";
import {
  CLOSING_WAIT,
  NOTICE_DURATION,
  frameUrl,
  fromFrame,
  hookPlatform,
  installed,
  pluginName,
  refreshPlugins,
  type FrameMessage,
  type HandedFile,
  type PluginHook,
  type PluginHookResult,
} from "../plugins";
import { hostOf, missingPermission, permissionsOf, withPermission, type PermissionNeed } from "../permissions";
import { offerExactAlarms } from "../exact-alarms";
import { pluginTheme } from "../theme";
import type { ContactAsk, Proposal } from "../pending-send";

// The plugin runs in its own frame and scheme and never sees the app's window, the chat or the keys.
// The core keeps each session apart and the plugin never learns which session or contact it is in.
const props = withDefaults(
  defineProps<{
    plugin: { id: string; name: string };
    contact: string;
    text?: string;
    sending?: Sending;
    file?: HandedFile;
    reference?: string;
    reminder?: string;
    live?: boolean;
    session?: string;
    fill?: boolean;
    presenting?: "lead" | "follow";
    hidden?: boolean;
    hook?: PluginHookName;
  }>(),
  {
    text: undefined,
    sending: "nothing",
    file: undefined,
    reference: undefined,
    reminder: undefined,
    live: false,
    session: undefined,
    fill: false,
    presenting: undefined,
    hidden: false,
    hook: undefined,
  },
);
const emit = defineEmits<{
  text: [text: string];
  attach: [file: PickedFile];
  pick: [ask: ContactAsk];
  sendTo: [contact: string, proposal: Proposal | null];
  done: [];
  openChat: [contact: string];
  closed: [];
  needs: [need: PermissionNeed];
  refused: [];
  shown: [visible: boolean];
}>();

const frame = ref<HTMLIFrameElement | null>(null);
const notices = ref<HTMLElement | null>(null);
const pane = ref<HTMLElement | null>(null);
const height = ref(320);
const late = ref(0);
const working = computed(() => late.value > 0);

function tell(message: Record<string, unknown>) {
  frame.value?.contentWindow?.postMessage(message, "*");
}

async function onMessage(event: MessageEvent) {
  const said = fromFrame(event, frame.value);
  if (!said) return;
  if (closing && SILENT_WHILE_CLOSING.has(said.type)) return;
  if (props.hidden && UNSAID_WHEN_HIDDEN.has(said.type)) return;
  if (props.hidden && !shown.value && NEEDS_EYES.has(said.type)) return refuseUnseen(said);

  if (said.type === "ft.hello") {
    tell({ type: "ft.theme", ...look() });
  } else if (said.type === "ft.ready") {
    ready = true;
    tell({ ...opening(), ...(await chatOf()) });
    markOpen();
  } else if (said.type === "ft.hookDone") {
    hookDone(said.id, said.result);
  } else if (said.type === "ft.show") {
    if (props.hidden && !shown.value) {
      shown.value = true;
      emit("shown", true);
    }
  } else if (said.type === "ft.height") {
    height.value = Math.min(Math.max(said.height, 160), 4000);
  } else if (said.type === "ft.pickFile") {
    // the plugin never opens the picker itself, the app asks the user
    await busy(async () => {
      try {
        const file = await pickForPlugin(said.accept ?? "");
        tell({ type: "ft.file", id: said.id, name: file?.name ?? "", mime: file?.mime ?? "", data: file?.data ?? "" });
      } catch {
        tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
      }
    });
  } else if (said.type === "ft.takePhoto") {
    await busy(async () => {
      try {
        const photo = await takePhotoForPlugin();
        tell({ type: "ft.file", id: said.id, name: photo?.name ?? "", mime: photo?.mime ?? "", data: photo?.data ?? "" });
      } catch {
        tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
      }
    });
  } else if (said.type === "ft.made") {
    if (!(await mayWrite())) return;
    const contact = props.contact || (await pickContact());
    if (!contact) return;
    await busy(async () => {
      try {
        const made = await pluginMade(props.plugin.id, contact, said.name, said.mime, said.data);
        if (!props.contact) return emit("sendTo", contact, made.staged ? { kind: "file", file: made.staged } : null);
        if (made.staged) emit("attach", made.staged);
        emit("done");
      } catch {
      }
    });
  } else if (said.type === "ft.text") {
    if (!(await mayWrite())) return;
    if (props.contact) return emit("text", said.text);
    const contact = await pickContact();
    if (contact) emit("sendTo", contact, { kind: "text", text: said.text });
  } else if (said.type === "ft.notify") {
    // without the permission the text never leaves the phone, only the user sees it in a toast
    void notify(said.text, said.sticky ? undefined : NOTICE_DURATION);
  } else if (said.type === "ft.close") {
    // a goodbye that ends with ft.close() asks again while already closing
    if (!closing) emit("done");
  } else if (said.type === "ft.closed") {
    letGo?.();
  } else {
    await answer(said);
  }
}

async function mayWrite(): Promise<boolean> {
  if (sending.value !== "nothing") return true;
  if (lacks("send")) return obtain("send");
  void mayNotWrite();
  return false;
}

async function mayNotWrite() {
  if (!props.contact) return;
  await notify(i18n.global.t("plugins.mayNotWrite"), 4000);
}

let notice: HTMLIonToastElement | undefined;
let noticing = Promise.resolve();

function notify(message: string, duration?: number): Promise<void> {
  noticing = noticing.then(async () => {
    const before = notice;
    notice = undefined;
    await before?.dismiss().catch(() => undefined);
    if (!message || closing) return;
    const anchor = notices.value?.offsetParent ? notices.value : undefined;
    notice = await toastController.create({ message, duration, position: "top", positionAnchor: anchor });
    fitToPane(notice);
    await notice.present();
  }).catch(() => undefined);
  return noticing;
}

// Ionic places the toast between --start and --end, move them to the pane's edges on a tablet
function fitToPane(toast: HTMLElement) {
  if (!pane.value) return;
  const { left, right } = pane.value.getBoundingClientRect();
  const gutter = document.documentElement.getAttribute("mode") === "ios" ? 10 : 8;
  const fromLeft = Math.max(0, left) + gutter;
  const fromRight = Math.max(0, window.innerWidth - right) + gutter;
  const rtl = getComputedStyle(pane.value).direction === "rtl";
  toast.style.setProperty("--start", `${rtl ? fromRight : fromLeft}px`);
  toast.style.setProperty("--end", `${rtl ? fromLeft : fromRight}px`);
}

// a closed plugin gets ft.closing so its twin hears it left, then goes after ft.closed or CLOSING_WAIT
let ready = false;
let closing: Promise<void> | undefined;
let letGo: (() => void) | undefined;
let gone = false;
const SILENT_WHILE_CLOSING = new Set<FrameMessage["type"]>([
  "ft.pickFile",
  "ft.takePhoto",
  "ft.made",
  "ft.text",
  "ft.openChat",
  "ft.drive",
  "ft.print",
  "ft.save",
  "ft.location",
  "ft.notify",
  // may bring up the phone's own download sheet
  "ft.translateLanguages",
  "ft.translateDetect",
  "ft.translateStatus",
  "ft.translatePrepare",
  "ft.translateRemove",
  "ft.translateText",
]);

// out of sight these would open the phone's own screens, so refuse them at once
const NEEDS_EYES = new Set<FrameMessage["type"]>(["ft.pickFile", "ft.takePhoto", "ft.print", "ft.save"]);
// a hidden frame only speaks through its hooks' answers
const UNSAID_WHEN_HIDDEN = new Set<FrameMessage["type"]>(["ft.text", "ft.made", "ft.openChat"]);

const shown = ref(false);
const unseen = computed(() => props.hidden && !shown.value);

function refuseUnseen(said: FrameMessage) {
  if (!("id" in said)) return;
  if (said.type === "ft.pickFile" || said.type === "ft.takePhoto") tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
  else tell({ type: "ft.done", id: said.id, answer: refusal(said) });
}

const hooksRunning = new Map<string, (result: PluginHookResult | null) => void>();
let hooksHanded = 0;
let byUser = true;
const declined = new Set<string>();
let refusedByCore = false;
let markOpen = () => {};
const opened = new Promise<void>((resolve) => (markOpen = resolve));

function runHook(hook: PluginHook): Promise<PluginHookResult | null> {
  if (closing || gone || refusedByCore) return Promise.resolve(null);
  const id = `h${(hooksHanded += 1)}`;
  return new Promise((resolve) => {
    hooksRunning.set(id, resolve);
    void opened.then(() => {
      if (!hooksRunning.has(id)) return;
      byUser = hook.manual;
      tell({
        type: "ft.hook",
        id,
        name: hook.name,
        text: hook.text,
        chat: hook.chat,
        ref: hook.ref,
        lang: hook.lang,
        manual: hook.manual,
        platform: hookPlatform((name) => isPlatform(name)),
        ...(hook.answer ? { answer: true } : {}),
        // postMessage can't clone reactive state
        ...(hook.values ? { values: { ...hook.values } } : {}),
      });
    });
  });
}

function hookDone(id: string, result: PluginHookResult | null) {
  const answer = hooksRunning.get(id);
  if (!answer) return;
  hooksRunning.delete(id);
  answer(result);
  if (props.hidden && shown.value && !hooksRunning.size) {
    shown.value = false;
    emit("shown", false);
  }
}

function dropHooks() {
  for (const [id] of [...hooksRunning]) hookDone(id, null);
}

function close(): Promise<void> {
  closing ??= new Promise<void>((resolve) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    letGo = () => {
      clearTimeout(timer);
      letGo = undefined;
      if (!gone) emit("closed");
      resolve();
    };
    void notify("");
    refuseUnanswered();
    dropHooks();
    if (!ready) return letGo();
    timer = setTimeout(letGo, CLOSING_WAIT);
    tell({ type: "ft.closing" });
  });
  return closing;
}
defineExpose({ close, runHook });

function look() {
  return { ...pluginTheme(), fill: props.fill };
}

function opening() {
  return {
    type: "ft.open",
    text: props.text ?? "",
    ...look(),
    lang: i18n.global.locale.value,
    // postMessage can't clone a reactive proxy
    file: props.file ? { name: props.file.name, mime: props.file.mime, data: props.file.data } : null,
    ref: props.reference ?? null,
    reminder: props.reminder ?? null,
    live: Boolean(props.live && props.contact),
    ...(props.presenting ? { presenting: props.presenting } : {}),
    ...(props.hook ? { hook: props.hook } : {}),
  };
}

// the core's opaque id for this plugin, none for a blocked contact or a closed session
async function chatOf(): Promise<{ chat?: string }> {
  if (!props.contact) return {};
  try {
    const chat = await pluginChat(props.plugin.id, props.contact);
    return typeof chat === "string" ? { chat } : {};
  } catch {
    return {};
  }
}

async function answer(said: Extract<FrameMessage, { id: string }>) {
  for (const key of await needed(said)) {
    if (lacks(key) && !(await obtain(key))) return tell({ type: "ft.done", id: said.id, answer: refusal(said) });
  }
  await busy(async () => {
    try {
      let value: unknown = true;
      const id = props.plugin.id;
      const session = props.session;
      if (said.type === "ft.save") await pluginSave(said.name, said.mime, said.data);
      else if (said.type === "ft.print") await pluginPrint(id, said.name, said.mime, said.data);
      else if (said.type === "ft.fetch") value = await pluginFetch(id, said.url, said.method, said.headers, said.body);
      else if (said.type === "ft.read") value = await pluginRead(id, said.key, session);
      else if (said.type === "ft.write") await pluginWrite(id, said.key, said.value, session);
      else if (said.type === "ft.forget") await pluginForget(id, said.key, session);
      else if (said.type === "ft.recordGet") value = await pluginRecordGet(id, said.key, session);
      else if (said.type === "ft.recordSet") await pluginRecordSet(id, said.key, said.value, session);
      else if (said.type === "ft.recordForget") await pluginRecordForget(id, said.key, session);
      else if (said.type === "ft.recordKeys") value = await pluginRecordKeys(id, said.prefix, session);
      else if (said.type === "ft.recordUsage") value = await pluginRecordUsage(id, session);
      else if (said.type === "ft.remindSet") await remindSet(id, said.reminder, said.at, said.text, session);
      else if (said.type === "ft.remindCancel") value = await remindCancel(id, said.reminder, session);
      else if (said.type === "ft.remindList") value = await remindList(id, session);
      else if (said.type === "ft.liveSend") value = live.value && props.contact ? await pluginLiveSend(id, props.contact, said.data) : false;
      else if (said.type === "ft.openChat") {
        const target = await pluginOpenChat(id, said.ref, session);
        if (target) emit("openChat", target.contact);
        value = Boolean(target);
      } else if (said.type === "ft.drive") value = await drive(said);
      else if (said.type === "ft.location") value = await pluginLocation(id);
      else if (said.type === "ft.translateLanguages") value = await translateLanguages(id);
      else if (said.type === "ft.translateDetect") value = await translateDetect(id, said.text);
      else if (said.type === "ft.translateStatus") value = await translateStatus(id, said.from, said.to);
      else if (said.type === "ft.translatePrepare") value = await translatePrepare(id, said.from, said.to);
      else if (said.type === "ft.translateRemove") value = await translateRemove(id, said.from, said.to);
      else if (said.type === "ft.translateText") value = await translateText(id, said.text, said.from, said.to);
      tell({ type: "ft.done", id: said.id, answer: value ?? null });
    } catch {
      tell({ type: "ft.done", id: said.id, answer: refusal(said) });
    }
  });
}

function refusal(said: { type: string }): null | false | [] | "unsupported" {
  switch (said.type) {
    case "ft.location":
    case "ft.translateDetect":
    case "ft.translateText":
      return null;
    case "ft.translateLanguages":
      return [];
    case "ft.translateStatus":
      return "unsupported";
    default:
      return false;
  }
}

// what a plugin lacks comes from what the core says it asked for, never from an error message

const sending = ref<Sending>(props.sending);
const live = ref(props.live);
watch(
  () => props.sending,
  (now) => (sending.value = now),
);
watch(
  () => props.live,
  (now) => (live.value = now),
);

function lacks(key: string): boolean {
  const view = installed.value.find((one) => one.id === props.plugin.id);
  return Boolean(view && missingPermission(view, key));
}

async function needed(said: Extract<FrameMessage, { id: string }>): Promise<string[]> {
  switch (said.type) {
    case "ft.location":
      return ["location"];
    case "ft.translateLanguages":
    case "ft.translateDetect":
    case "ft.translateStatus":
    case "ft.translatePrepare":
    case "ft.translateRemove":
    case "ft.translateText":
      return ["translate"];
    case "ft.remindSet":
      return ["remind"];
    case "ft.liveSend":
      return props.contact && !live.value ? ["live"] : [];
    case "ft.fetch": {
      const host = hostOf(said.url);
      return host ? [`network:${host}`] : [];
    }
    case "ft.drive":
      if (said.op === "setup" || said.op === "unlock") return [];
      return said.op === "send" && sending.value === "nothing" ? ["drive", "send"] : ["drive"];
    case "ft.recordSet":
      return lacks("storage") && !(await fits(said.key, said.value)) ? ["storage"] : [];
    default:
      return [];
  }
}

const bytes = (text: string) => new TextEncoder().encode(text).length;

async function fits(key: string, value: string): Promise<boolean> {
  try {
    const { used, quota } = await pluginRecordUsage(props.plugin.id, props.session);
    const held = await pluginRecordGet(props.plugin.id, key, props.session);
    return used - bytes(held ?? "") + bytes(value) <= quota;
  } catch {
    return true;
  }
}

let picking: ((contact: string | null) => void) | undefined;
function pickContact(): Promise<string | null> {
  picking?.(null);
  if (closing || gone) return Promise.resolve(null);
  return new Promise((resolve) => {
    const answer = (contact: string | null) => {
      if (picking !== answer) return;
      picking = undefined;
      unanswered.delete(refuse);
      resolve(closing || gone ? null : contact);
    };
    const refuse = () => answer(null);
    picking = answer;
    unanswered.add(refuse);
    emit("pick", { answer });
  });
}

let questions: Promise<unknown> = Promise.resolve();
const asking = new Map<string, Promise<boolean>>();
const unanswered = new Set<(allowed: boolean) => void>();

function refuseUnanswered() {
  for (const answer of [...unanswered]) answer(false);
}

function obtain(key: string): Promise<boolean> {
  if (props.hidden && !byUser && declined.has(key)) return Promise.resolve(false);
  const already = asking.get(key);
  if (already) return already;
  const asked = questions.then(() => ask(key));
  questions = asked.catch(() => undefined);
  asking.set(key, asked);
  void asked.finally(() => asking.delete(key)).catch(() => undefined);
  return asked;
}

async function ask(key: string): Promise<boolean> {
  const id = props.plugin.id;
  if (closing || gone) return false;
  const view = (await refreshPlugins()).find((one) => one.id === id);
  const line = view && missingPermission(view, key);
  if (!view || !line) return Boolean(view && permissionsOf(view).some((one) => one.key === key && one.on));
  const yes = await new Promise<boolean>((resolve) => {
    const answer = (allowed: boolean) => {
      if (!unanswered.delete(answer)) return;
      resolve(allowed);
    };
    unanswered.add(answer);
    emit("needs", { ...line, answer });
  });
  if (!yes && props.hidden) declined.add(key);
  if (!yes || closing || gone) return false;
  try {
    await grantPlugin(id, withPermission(view, key, true));
  } catch {
    return false;
  }
  await refreshPlugins();
  if (lacks(key)) return false;
  if (key === "send") sending.value = installed.value.find((one) => one.id === id)?.granted.send ?? "nothing";
  if (key === "live") live.value = true;
  if (key === "remind") await offerExactAlarms();
  return true;
}


// the plugin sees names and sizes, never bytes, tokens or the code
async function drive(said: Extract<FrameMessage, { type: "ft.drive" }>): Promise<unknown> {
  const id = props.plugin.id;
  if (!(await pluginMayUseDrive(id))) return false;
  const parent = (value: string) => value || null;
  switch (said.op) {
    case "status":
      return vaultStatus();
    case "connect":
      return vaultConnect(said.a || "google");
    // a plugin can't set up or unlock the drive, so the phrase never crosses the frame
    case "setup":
    case "unlock":
      return false;
    case "disconnect":
      await vaultDisconnect();
      return true;
    case "list":
      return vaultList(parent(said.a));
    case "mkdir":
      return vaultMkdir(said.a, parent(said.b));
    case "rename":
      await vaultRename(said.a, said.b);
      return true;
    case "move":
      await vaultMove(said.a, parent(said.b));
      return true;
    case "remove":
      await vaultRemove(said.a);
      return true;
    case "upload":
      return vaultUploadPicked(parent(said.a));
    case "keep": {
      // by ref, the bytes never pass through the frame
      if (!props.reference) return false;
      const target = await pluginOpenChat(id, props.reference, props.session);
      if (!target) return false;
      await vaultUploadMessage(target.message, parent(said.a));
      return true;
    }
    case "open":
      await vaultOpen(said.a);
      return true;
    case "save":
      await vaultSave(said.a);
      return true;
    case "send": {
      if (sending.value === "nothing") return false;
      const contact = props.contact || (await pickContact());
      if (!contact) return false;
      const file = await vaultDownload(said.a);
      if (sending.value === "auto") await sendPicked(contact, file);
      if (!props.contact) emit("sendTo", contact, sending.value === "auto" ? null : { kind: "file", file });
      else {
        if (sending.value !== "auto") emit("attach", file);
        emit("done");
      }
      return true;
    }
    case "retry":
      return vaultRetry();
    case "cancel":
      await vaultCancelPending(said.a);
      return true;
    case "backup":
      return vaultBackup();
    case "backupInfo":
      return vaultBackupInfo();
    case "restore":
      return vaultRestore();
    default:
      return false;
  }
}

// only this plugin and this conversation
function onLive(event: PluginEvent) {
  if (live.value && event.plugin === props.plugin.id && event.contact === props.contact) {
    tell({ type: "ft.live", data: event.data });
  }
}
let unlisten: (() => void) | undefined;

// most answers take tens of ms, an indicator for each made the plugin flicker
const WORKING_DELAY = 400;

async function busy(work: () => Promise<void>) {
  let slow = false;
  const timer = setTimeout(() => {
    slow = true;
    late.value++;
  }, WORKING_DELAY);
  try {
    await work();
  } finally {
    clearTimeout(timer);
    if (slow) late.value--;
  }
}

let lastTheme = "";
const looks = new MutationObserver(() => {
  const now = look();
  const said = JSON.stringify(now);
  if (said === lastTheme) return;
  lastTheme = said;
  tell({ type: "ft.theme", ...now });
});

const reportOpen = (id: string, open: boolean) => void pluginOpen(id, open).catch(() => undefined);
watch(
  () => props.plugin.id,
  (now, before) => {
    reportOpen(before, false);
    reportOpen(now, true);
  },
);

onMounted(async () => {
  void pluginOpen(props.plugin.id, true).catch(() => {
    refusedByCore = true;
    dropHooks();
    emit("refused");
  });
  lastTheme = JSON.stringify(look());
  looks.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-direction", "dir"] });
  window.addEventListener("message", onMessage);
  unlisten = await listen<PluginEvent>(PLUGIN_EVENT, ({ payload }) => onLive(payload)).catch(() => undefined);
});
onBeforeUnmount(() => {
  reportOpen(props.plugin.id, false);
  gone = true;
  if (ready && !closing) tell({ type: "ft.closing" });
  letGo?.();
  refuseUnanswered();
  dropHooks();
  void notify("");
  looks.disconnect();
  window.removeEventListener("message", onMessage);
  // no listeners in tests and on desktop
  void Promise.resolve()
    .then(() => unlisten?.())
    .catch(() => undefined);
});
watch(
  () => [props.text, props.reminder],
  () => tell(opening()),
);
</script>

<template>
  <section
    ref="pane"
    class="ft-plugin"
    :class="{ 'ft-plugin--fill': fill, 'is-presenting': presenting, 'ft-plugin--hidden': unseen }"
    :inert="unseen || undefined"
    :aria-hidden="unseen || undefined"
  >
    <span v-if="working" class="ft-plugin__working" role="status">…</span>
    <span ref="notices" class="ft-plugin__notices" aria-hidden="true" />
    <iframe
      ref="frame"
      class="ft-plugin__frame"
      :src="frameUrl(plugin.id)"
      :style="fill || presenting ? undefined : { height: `${height}px` }"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      :title="pluginName(plugin)"
    />
  </section>
</template>

<style scoped>
.ft-plugin {
  position: relative;
  padding: 0 var(--ft-space-4) var(--ft-space-4);
}
.ft-plugin--fill {
  display: flex;
  flex-direction: column;
  height: 100%;
  padding-bottom: 0;
}
.ft-plugin--fill .ft-plugin__frame {
  flex: 1;
  min-height: 0;
}
.ft-plugin--hidden {
  visibility: hidden;
  pointer-events: none;
}
.ft-plugin__working {
  position: absolute;
  z-index: 1;
  inset-block-start: var(--ft-space-2);
  inset-inline-end: var(--ft-space-4);
  padding: 0 var(--ft-space-3);
  border-radius: 999px;
  background: var(--ion-color-light);
  color: var(--ion-color-primary);
  pointer-events: none;
}
.ft-plugin__notices {
  position: sticky;
  inset-block-start: 0;
  display: block;
  height: 0;
}
.ft-plugin__frame {
  display: block;
  width: 100%;
  border: 0;
  background: transparent;
}
.ft-plugin.is-presenting {
  height: 100%;
  padding-block-end: 0;
}
.is-presenting .ft-plugin__frame {
  height: 100%;
}
</style>
