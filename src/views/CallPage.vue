<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch, watchEffect } from "vue";
import { IonActionSheet, IonButton, IonContent, IonIcon, IonPage, IonSpinner, onIonViewWillLeave, toastController } from "@ionic/vue";
import {
  callOutline,
  cameraReverseOutline,
  chevronDown,
  documentOutline,
  downloadOutline,
  easelOutline,
  headsetOutline,
  micOffOutline,
  micOutline,
  pauseCircleOutline,
  phonePortraitOutline,
  stopCircleOutline,
  videocamOffOutline,
  videocamOutline,
  volumeHighOutline,
} from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import GamePermissions from "../components/GamePermissions.vue";
import PluginSheet from "../components/PluginSheet.vue";
import { closeOnBackWhile, goBack } from "../back";
import { callScreenMounted } from "../call-screen";
import { nearestCorner, thumbArea } from "../call-thumbnail";
import { useOnScreen } from "../on-screen";
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
  sessionOf,
} from "../core";
import { installed, isLocked, pluginIcon, pluginImage, pluginName, refreshPlugins, refreshPremiumLock, type HandedFile } from "../plugins";
import {
  PRESENT_BOARD,
  PRESENT_DOCUMENT,
  PRESENT_FILE_LIMIT,
  SEND_WAIT,
  canPresentWith,
  holeClip,
  installOffered,
  needsPresentGrant,
  presentGrant,
  sentFile,
  waitFor,
} from "../present";
import {
  call,
  cannotPresent,
  hangUp,
  hideVideo,
  layoutVideo,
  presentInCall,
  rectOf,
  startCall,
  stopPresenting,
  switchCamera,
  toggleCamera,
  toggleMute,
  toggleSpeaker,
  type VideoLayout,
} from "../calls";
import { t } from "../i18n";
import { darkScreen } from "../theme";

const route = useRoute();
const router = useRouter();

const id = computed(() => String(route.params.id));
const current = computed(() => call.contact === id.value && call.phase !== "idle");
const contact = computed(() => chat(id.value) ?? { name: "", hue: 0 });
const isVideo = computed(() => (current.value ? call.video : Boolean(route.query.video)));

const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;
const onScreen = useOnScreen(cameIn);

const pad = (value: number) => String(value).padStart(2, "0");
const state = computed(() => {
  switch (call.phase) {
    case "active": {
      const seconds = Math.max(0, Math.floor((now.value - call.since) / 1000));
      return `${pad(Math.floor(seconds / 60))}:${pad(seconds % 60)}`;
    }
    case "ended":
      return t(`calls.outcomes.${call.outcome ?? "ended"}`);
    case "calling":
      return t("calls.calling");
    default:
      return t("calls.connecting");
  }
});

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

interface Asking {
  them: boolean;
  name: string;
  body: string;
  icon?: string;
  image?: string;
  allow: () => Promise<void>;
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

type PresentState = "none" | "missing" | "ask" | "declined" | "accept" | "loading" | "failed" | "tooBig" | "sheet";
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
const presentClip = ref<string | undefined>();
// measured for the hole, so no CSS transform on it
const presentBox = ref<HTMLElement | null>(null);
const controls = ref<HTMLElement | null>(null);
const presentBottom = ref<number | undefined>();
const presentStyle = computed(() => {
  const style: Record<string, string> = {};
  if (presentClip.value) style.clipPath = presentClip.value;
  if (presentBottom.value !== undefined) style.bottom = `${presentBottom.value}px`;
  return Object.keys(style).length ? style : undefined;
});

watch(
  () => Boolean(call.presenting),
  (now) => {
    if (!now) return;
    choosing.value = false;
    if (asking.value && !asking.value.them) asking.value = null;
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

// kept after its presentation until it says goodbye, so both phones hear ft.onClose
interface Shown {
  key: string;
  plugin: { id: string; name: string };
  file?: HandedFile;
  lead: boolean;
}
// a new round mounts a fresh sheet instead of patching the closing one
const onSheet = ref<(Shown & { round: number }) | null>(null);
let sheetRounds = 0;
const sheetLeaving = ref(false);
const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
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

// on phones the pictures are native views under the WebView, this page leaves holes for them
const native = computed(() => current.value && call.native);
const live = computed(() => call.phase === "active");
const cameraReady = computed(() => live.value && call.view.available);
// before the video line is up a see-through page only shows the window's white
const cameraRunning = computed(() => cameraReady.value && call.view.camera);
const stage = computed(() => native.value && live.value && (cameraRunning.value || call.view.remote));
const showRemote = computed(() => stage.value && call.view.remote);
const showLocal = computed(() => stage.value && cameraRunning.value && !presentArea.value);
const localThumb = computed(() => Boolean(showRemote.value && showLocal.value));
const remoteThumb = computed(() => Boolean(showRemote.value && presentArea.value));
const invite = computed(() => native.value && cameraReady.value && call.view.remote && !call.view.camera);
const dark = computed(() => native.value && !stage.value && (call.video || call.view.camera || call.view.remote));
const cameraUsable = computed(() => cameraReady.value || (native.value && call.view.camera));

const unavailable = ref(false);
let unavailableTimer: ReturnType<typeof setTimeout> | undefined;
function camera() {
  if (!native.value || cameraUsable.value) {
    void toggleCamera();
    return;
  }
  if (!live.value) return;
  unavailable.value = true;
  clearTimeout(unavailableTimer);
  unavailableTimer = setTimeout(() => (unavailable.value = false), 4000);
}

function flip() {
  if (cameraRunning.value) void switchCamera();
}

const body = ref<HTMLElement | null>(null);
const remoteSlot = ref<HTMLElement | null>(null);
const localSlot = ref<HTMLElement | null>(null);

function measure(): VideoLayout {
  return {
    remote: rectOf(remoteSlot.value),
    local: rectOf(localSlot.value),
    mirrorLocal: call.view.facing === "front",
    localRadius: localThumb.value ? 16 : 0,
  };
}
let shown = true;
closeOnBackWhile(() => onScreen.value && choosing.value, () => (choosing.value = false));
closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => answerAsk(false));
function measureControls() {
  const screen = rectOf(body.value);
  const buttons = rectOf(controls.value);
  if (!screen || !buttons || screen.height <= 0 || buttons.height <= 0) return;
  presentBottom.value = Math.max(0, Math.round(screen.y + screen.height - buttons.y));
}
function relayout() {
  measureControls();
  presentClip.value = remoteThumb.value ? holeClip(rectOf(presentBox.value), rectOf(remoteSlot.value)) : undefined;
  if (shown && native.value) layoutVideo(measure);
}
function hide() {
  shown = false;
  if (native.value) hideVideo();
}

const seeThrough = ref(true);
watchEffect(() => document.documentElement.classList.toggle("ft-call-video", Boolean(stage.value) && seeThrough.value));
watchEffect(() => darkScreen("call", onScreen.value && Boolean(stage.value || dark.value)));

watch([native, stage, showRemote, showLocal, remoteThumb, presentArea, () => call.view.facing], relayout, { flush: "post" });
const sized = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(relayout);
watch([body, remoteSlot, localSlot, presentBox], (elements) => {
  sized?.disconnect();
  for (const element of elements) if (element) sized?.observe(element);
});
function cameIn() {
  shown = true;
  seeThrough.value = true;
  relayout();
}
onIonViewWillLeave(() => {
  clearTimeout(leaving);
  hide();
  seeThrough.value = false;
});

const drag = reactive({ x: 0, y: 0 });
let grab: { id: number; x: number; y: number; fromX: number; fromY: number; slot: Element } | null = null;
const dragging = ref(false);
const SETTLE_MS = 200;
let settling = 0;
const minimize = ref<{ $el?: Element } | null>(null);
const peer = ref<HTMLElement | null>(null);
const thumbStyle = computed(() => ((localThumb.value || remoteThumb.value) && (drag.x || drag.y) ? { transform: `translate(${drag.x}px, ${drag.y}px)` } : undefined));
watch([localThumb, remoteThumb], () => {
  endDrag();
  Object.assign(drag, { x: 0, y: 0 });
});
watch(thumbStyle, relayout, { flush: "post" });
const thumbSlot = () => (remoteThumb.value ? remoteSlot.value : localThumb.value ? localSlot.value : null);
function grabThumb(event: PointerEvent) {
  if (!thumbSlot() || event.currentTarget !== thumbSlot()) return;
  const slot = event.currentTarget as Element;
  endDrag();
  grab = { id: event.pointerId, x: event.clientX, y: event.clientY, fromX: drag.x, fromY: drag.y, slot };
  dragging.value = true;
  slot.setPointerCapture?.(event.pointerId);
  // on the iPhone the slot stopped getting pointer events after a few points
  window.addEventListener("pointermove", moveThumb);
  window.addEventListener("pointerup", dropThumb);
  window.addEventListener("pointercancel", dropThumb);
}
function endDrag() {
  cancelAnimationFrame(settling);
  settling = 0;
  window.removeEventListener("pointermove", moveThumb);
  window.removeEventListener("pointerup", dropThumb);
  window.removeEventListener("pointercancel", dropThumb);
  dragging.value = false;
  if (!grab) return;
  const { slot, id } = grab;
  grab = null;
  try {
    slot.releasePointerCapture?.(id);
  } catch {
    // pointer already gone after a cancelled touch
  }
}
function moveThumb(event: PointerEvent) {
  if (!grab || event.pointerId !== grab.id) return;
  drag.x = grab.fromX + event.clientX - grab.x;
  drag.y = grab.fromY + event.clientY - grab.y;
  relayout();
}
function dropThumb(event: PointerEvent) {
  if (!grab || event.pointerId !== grab.id) return;
  endDrag();
  const box = thumbSlot()?.getBoundingClientRect();
  const screen = body.value?.getBoundingClientRect();
  if (box && screen && screen.width > 0 && screen.height > 0) {
    const home = { left: box.left - drag.x, top: box.top - drag.y, right: box.right - drag.x, bottom: box.bottom - drag.y };
    const above = [rectOf(minimize.value?.$el), rectOf(peer.value)].map((rect) => (rect ? rect.y + rect.height : screen.top));
    const corner = nearestCorner(box, thumbArea(screen, home, Math.max(screen.top, ...above)));
    settle({ x: Math.round(drag.x + corner.x - box.left), y: Math.round(drag.y + corner.y - box.top) });
  }
  relayout();
}

// one step per frame, the native view can't follow a CSS transition
function settle(to: { x: number; y: number }) {
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    Object.assign(drag, to);
    return;
  }
  const from = { x: drag.x, y: drag.y };
  let start: number | undefined;
  const step = (now: number) => {
    start ??= now;
    const done = Math.min(1, (now - start) / SETTLE_MS);
    const eased = 1 - (1 - done) ** 3;
    Object.assign(drag, done < 1 ? { x: from.x + (to.x - from.x) * eased, y: from.y + (to.y - from.y) * eased } : to);
    settling = done < 1 ? requestAnimationFrame(step) : 0;
  };
  settling = requestAnimationFrame(step);
}

const remoteVideo = ref<HTMLVideoElement | null>(null);
const localVideo = ref<HTMLVideoElement | null>(null);
const remoteAudio = ref<HTMLAudioElement | null>(null);
watchEffect(() => {
  if (remoteVideo.value) remoteVideo.value.srcObject = call.remote;
  if (localVideo.value) localVideo.value.srcObject = call.local;
  if (remoteAudio.value) remoteAudio.value.srcObject = call.remote;
});

let unmounted: (() => void) | undefined;
onMounted(() => {
  unmounted = callScreenMounted(router);
  void readPlugins();
  void refreshPremiumLock();
  if (!current.value || call.phase === "ended") void startCall(id.value, Boolean(route.query.video));
  ticking = setInterval(() => (now.value = Date.now()), 1000);
  window.addEventListener("resize", relayout);
  window.addEventListener("orientationchange", relayout);
  relayout();
});
let left = false;
let leaving: ReturnType<typeof setTimeout> | undefined;
function leave(): Promise<unknown> | undefined {
  if (left) return;
  left = true;
  clearTimeout(leaving);
  return window.history.state?.back ? goBack(router) : router.replace(`/chat/${id.value}`);
}
closeOnBackWhile(() => onScreen.value, leave);
function end() {
  if (call.phase !== "idle" && call.phase !== "ended") void hangUp();
  void leave();
}

onUnmounted(() => {
  unmounted?.();
  clearInterval(ticking);
  clearTimeout(unavailableTimer);
  window.removeEventListener("resize", relayout);
  window.removeEventListener("orientationchange", relayout);
  sized?.disconnect();
  endDrag();
  hide();
  seeThrough.value = false;
  document.documentElement.classList.remove("ft-call-video");
  darkScreen("call", false);
  left = true;
  clearTimeout(leaving);
});
watch(
  () => call.phase,
  (phase) => {
    if (phase !== "active") {
      choosing.value = false;
      if (asking.value && !asking.value.them) asking.value = null;
    }
    if (phase === "ended" && onScreen.value) leaving = setTimeout(leave, 1500);
  },
);
</script>

<template>
  <ion-page>
    <ion-content class="ft-call" :class="{ 'is-dark': dark }">
      <div
        ref="body"
        class="ft-call__body"
        :class="native ? { 'is-video': stage, 'is-live': stage } : { 'is-video': isVideo, 'is-live': Boolean(call.remote) }"
      >
        <ion-button
          v-if="call.phase !== 'ended'"
          ref="minimize"
          class="ft-call__minimize"
          fill="clear"
          shape="round"
          :aria-label="$t('calls.minimize')"
          @click="leave"
        >
          <ion-icon slot="icon-only" :icon="chevronDown" aria-hidden="true" />
        </ion-button>
        <ion-button
          v-if="canOffer"
          class="ft-call__end ft-call__end--clear"
          fill="clear"
          shape="round"
          data-test="present"
          :disabled="starting"
          :aria-label="$t('calls.present')"
          @click="choosing = !starting"
        >
          <ion-icon slot="icon-only" :icon="easelOutline" aria-hidden="true" />
        </ion-button>
        <ion-button
          v-else-if="presenting?.by === 'me'"
          class="ft-call__end"
          color="danger"
          shape="round"
          data-test="stop-presenting"
          :aria-label="$t('calls.stopPresenting')"
          @click="stop"
        >
          <ion-icon slot="icon-only" :icon="stopCircleOutline" aria-hidden="true" />
        </ion-button>
        <p v-if="presenting?.by === 'them'" class="ft-call__presenter" data-test="presenter" dir="auto">
          <ion-button
            v-if="presentState === 'declined'"
            fill="clear"
            size="small"
            color="medium"
            data-test="presenter-ask"
            :aria-label="$t('calls.presentAsk', { name: contact.name, plugin: presentPlugin ? pluginName(presentPlugin) : '' })"
            @click="declined = ''"
          >
            {{ $t("calls.presenting", { name: contact.name }) }}
          </ion-button>
          <template v-else>{{ $t("calls.presenting", { name: contact.name }) }}</template>
        </p>
        <section
          v-if="presentArea || onSheet"
          v-show="presentArea"
          ref="presentBox"
          class="ft-call__present"
          :class="{ 'is-dragging': dragging }"
          :style="presentStyle"
          data-test="present-area"
        >
          <PluginSheet
            v-if="onSheet"
            v-show="!sheetLeaving"
            ref="sheet"
            :key="`${onSheet.key}#${onSheet.round}`"
            :plugin="onSheet.plugin"
            :contact="id"
            :live="true"
            :file="onSheet.file"
            :session="sessionOf(id)"
            :presenting="onSheet.lead ? 'lead' : 'follow'"
            fill
            @done="presentDone"
            @refused="presentRefused"
            @closed="sheetClosed"
          />
          <div v-if="!onSheet || sheetLeaving" class="ft-call__present-wait">
            <ion-button v-if="presentState === 'accept'" shape="round" data-test="present-accept" @click="acceptPresented">
              <ion-icon slot="start" :icon="downloadOutline" aria-hidden="true" />
              {{ $t("calls.acceptToSee") }}
            </ion-button>
            <p v-else-if="presentState === 'failed'" class="ft-call__notice" role="alert" data-test="present-failed">{{ $t("calls.presentFailed") }}</p>
            <p v-else-if="presentState === 'tooBig'" class="ft-call__notice" role="alert" data-test="present-too-big">{{ $t("calls.presentTooBig") }}</p>
            <ion-spinner v-else name="crescent" color="medium" data-test="present-loading" role="status" :aria-label="waitLabel" />
          </div>
        </section>
        <div v-if="native" class="ft-call__stage" :data-test="stage ? 'video' : undefined">
          <div
            v-if="showRemote"
            ref="remoteSlot"
            class="ft-call__slot ft-call__slot--remote"
            :class="{ 'is-thumb': remoteThumb }"
            :style="remoteThumb ? thumbStyle : undefined"
            data-test="remote-slot"
            @pointerdown="grabThumb"
            @pointermove="moveThumb"
            @pointerup="dropThumb"
            @pointercancel="dropThumb"
          >
            <span v-if="call.view.remotePaused && remoteThumb" class="ft-call__paused" :aria-label="$t('calls.cameraPaused')" role="img" data-test="remote-paused">
              <ion-icon :icon="pauseCircleOutline" aria-hidden="true" />
            </span>
            <p v-else-if="call.view.remotePaused" class="ft-call__paused" data-test="remote-paused">
              <ion-icon :icon="pauseCircleOutline" aria-hidden="true" />
              {{ $t("calls.cameraPaused") }}
            </p>
          </div>
          <div
            v-if="showLocal"
            ref="localSlot"
            class="ft-call__slot ft-call__slot--local"
            :class="{ 'is-thumb': localThumb }"
            :style="localThumb ? thumbStyle : undefined"
            data-test="local-slot"
            @pointerdown="grabThumb"
            @pointermove="moveThumb"
            @pointerup="dropThumb"
            @pointercancel="dropThumb"
          >
            <span v-if="call.view.paused" class="ft-call__paused" :aria-label="$t('calls.cameraPaused')" role="img">
              <ion-icon :icon="pauseCircleOutline" aria-hidden="true" />
            </span>
          </div>
        </div>
        <div v-else-if="isVideo" class="ft-call__video" data-test="video">
          <video v-if="call.remote" ref="remoteVideo" class="ft-call__remote" autoplay playsinline />
          <video v-show="call.local" ref="localVideo" class="ft-call__self" autoplay playsinline muted />
        </div>
        <audio v-else ref="remoteAudio" autoplay />

        <div ref="peer" class="ft-call__peer" :style="presentArea ? { visibility: 'hidden' } : undefined">
          <Avatar v-if="native ? !stage : !isVideo || !call.remote" :name="contact.name" :hue="contact.hue" :size="132" />
          <h1 class="ft-call__name" dir="auto">{{ contact.name }}</h1>
          <span class="ft-call__state" :class="{ 'is-live': call.phase === 'active' }">{{ state }}</span>
        </div>

        <div class="ft-call__notices">
          <p v-if="call.cameraDenied" class="ft-call__notice" role="alert" data-test="camera-denied">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.cameraDenied") }}
          </p>
          <p v-if="call.cameraFailed" class="ft-call__notice" role="alert" data-test="camera-failed">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.cameraFailed") }}
          </p>
          <p v-if="unavailable" class="ft-call__notice" role="status">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.videoUnavailable") }}
          </p>
          <p v-if="invite && !presentArea" class="ft-call__notice ft-call__notice--invite">
            <ion-icon :icon="videocamOutline" aria-hidden="true" />
            {{ $t("calls.turnOnCamera") }}
          </p>
          <p v-if="presentState === 'missing'" class="ft-call__notice" role="status" data-test="present-missing">
            <ion-icon :icon="easelOutline" aria-hidden="true" />
            {{ $t("calls.presentMissing") }}
          </p>
        </div>

        <div ref="controls" class="ft-call__controls">
          <button
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': call.muted }"
            :aria-label="$t('calls.mute')"
            :aria-pressed="call.muted"
            @click="toggleMute"
          >
            <ion-icon
              :key="call.muted ? 'mic-off' : 'mic-on'"
              :icon="call.muted ? micOffOutline : micOutline"
              aria-hidden="true"
            />
          </button>
          <button
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': call.speaker }"
            :aria-label="$t('calls.speaker')"
            :aria-pressed="call.speaker"
            @click="toggleSpeaker"
          >
            <ion-icon
              :key="call.speaker ? 'speaker-on' : call.route === 'headset' ? 'speaker-headset' : 'speaker-off'"
              :icon="call.speaker ? volumeHighOutline : call.route === 'headset' ? headsetOutline : phonePortraitOutline"
              aria-hidden="true"
            />
          </button>
          <button
            v-if="native"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': call.view.camera, 'is-invite': invite, 'is-waiting': !cameraUsable }"
            :aria-label="$t('calls.camera')"
            :aria-pressed="call.view.camera"
            :aria-disabled="!cameraUsable"
            @click="camera"
          >
            <ion-icon
              :key="call.view.camera ? 'camera-on' : 'camera-off'"
              :icon="call.view.camera ? videocamOutline : videocamOffOutline"
              aria-hidden="true"
            />
          </button>
          <button
            v-else-if="isVideo"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': call.cameraOff }"
            :aria-label="$t('calls.camera')"
            :aria-pressed="!call.cameraOff"
            @click="toggleCamera"
          >
            <ion-icon
              :key="call.cameraOff ? 'camera-off' : 'camera-on'"
              :icon="call.cameraOff ? videocamOffOutline : videocamOutline"
              aria-hidden="true"
            />
          </button>
          <button
            v-if="native"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-waiting': !cameraRunning }"
            :aria-label="$t('calls.switchCamera')"
            :aria-disabled="!cameraRunning"
            @click="flip"
          >
            <ion-icon :icon="cameraReverseOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-call__hangup" :aria-label="$t('calls.hangUp')" @click="end">
            <ion-icon :icon="callOutline" aria-hidden="true" />
          </button>
        </div>
      </div>
      <!-- Ionic moves an open sheet to the app root, so Vue must never use it as an anchor. The patch threw and the screen stopped redrawing. -->
      <ion-action-sheet :is-open="choosing" :header="$t('calls.present')" :buttons="presentButtons" @did-dismiss="choosing = false" />
      <GamePermissions
        :open="Boolean(asking)"
        :name="asking?.name ?? ''"
        :icon="asking?.icon"
        :image="asking?.image"
        :body="asking?.body"
        :allow-label="$t('calls.presentAllow')"
        @allow="answerAsk(true)"
        @cancel="answerAsk(false)"
      />
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-call {
  --background: var(--ft-bg);
}
.ft-call.is-dark {
  --ft-bg: #07090c;
  --ft-text: #f4f5f7;
  --ft-muted: rgba(244, 245, 247, 0.65);
  --ft-surface-2: rgba(255, 255, 255, 0.12);
  --background: #07090c;
  color: var(--ft-text);
}
.ft-call__body {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  min-height: 100%;
  padding: calc(env(safe-area-inset-top) + var(--ft-space-5)) var(--ft-space-4)
    calc(var(--ion-safe-area-bottom, 0px) + var(--ft-space-5));
}

.ft-call__minimize {
  position: absolute;
  z-index: 1;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline-start: calc(max(env(safe-area-inset-left), env(safe-area-inset-right)) + var(--ft-space-2));
  /* 44 pt, Apple's minimum tap target */
  min-width: 44px;
  min-height: 44px;
  /* a coloured clear button drops its background, and this one needs one over the pictures */
  --color: var(--ion-color-medium);
  --background: rgba(var(--ion-color-medium-rgb), 0.18);
}
.ft-call__end {
  position: absolute;
  z-index: 2;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline-end: calc(max(env(safe-area-inset-left), env(safe-area-inset-right)) + var(--ft-space-2));
  min-width: 44px;
  min-height: 44px;
}
.ft-call__end--clear {
  --color: var(--ion-color-medium);
  --background: rgba(var(--ion-color-medium-rgb), 0.18);
}
.ft-call__presenter {
  position: absolute;
  z-index: 2;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline: 60px;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  margin: 0;
  color: var(--ion-color-medium);
  font-size: 14px;
}
.ft-call__present {
  position: absolute;
  z-index: 1;
  inset-inline: 0;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2) * 2 + 44px);
  bottom: calc(var(--ion-safe-area-bottom, 0px) + var(--ft-space-5) * 2 + 58px);
  overflow-y: auto;
  background: var(--ion-background-color);
}
.ft-call__present.is-dragging {
  pointer-events: none;
}
.ft-call__present-wait {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  padding: var(--ft-space-4);
}

.ft-call__video {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(90% 60% at 50% 20%, color-mix(in srgb, var(--ft-accent) 18%, transparent), transparent 70%),
    var(--ft-surface-2);
}
.ft-call__remote {
  width: 100%;
  height: 100%;
  background: #000;
  object-fit: contain;
}
.ft-call__self {
  object-fit: cover;
  transform: scaleX(-1);
  position: absolute;
  inset-inline-end: var(--ft-space-4);
  bottom: calc(var(--ion-safe-area-bottom, 0px) + 110px);
  width: 96px;
  height: 140px;
  border: 1px solid var(--ft-border);
  border-radius: 16px;
  background: var(--ft-surface);
  box-shadow: 0 12px 30px -16px rgba(0, 0, 0, 0.8);
}

.ft-call__peer {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-3);
  margin-top: 12vh;
  text-align: center;
}
.is-video.is-live .ft-call__peer {
  margin-top: 0;
  padding: 10px 18px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--ft-bg) 55%, transparent);
  backdrop-filter: blur(10px);
}
.ft-call__name {
  margin: 0;
  font-size: 26px;
  font-weight: 700;
}
.ft-call__state {
  color: var(--ft-muted);
  font-size: 14px;
  font-variant-numeric: tabular-nums;
}
.ft-call__state.is-live {
  color: var(--ft-accent);
}

.ft-call__controls {
  position: relative;
  z-index: 2;
  display: flex;
  align-items: center;
  gap: var(--ft-space-3);
}
.ft-call__controls .ft-round {
  width: clamp(48px, 14vw, 58px);
  height: clamp(48px, 14vw, 58px);
  font-size: 26px;
}
.ft-round.is-waiting {
  opacity: 0.45;
}
/* their camera coming on only offers mine, never turns it on */
.ft-round.is-invite {
  background: var(--ft-accent);
  color: var(--ft-on-accent);
  animation: ft-call-invite 1.6s ease-in-out infinite;
}
@keyframes ft-call-invite {
  50% {
    box-shadow: 0 0 0 8px color-mix(in srgb, var(--ft-accent) 30%, transparent);
  }
}
@media (prefers-reduced-motion: reduce) {
  .ft-round.is-invite {
    animation: none;
  }
}

.ft-call__stage {
  position: absolute;
  inset: 0;
}
.ft-call__slot {
  position: absolute;
  background: transparent;
}
.ft-call__slot--remote,
.ft-call__slot--local {
  inset: 0;
}
/* a SurfaceView is never rounded on Android */
.ft-call__slot.is-thumb {
  inset: auto;
  inset-inline-end: var(--ft-space-4);
  bottom: calc(var(--ion-safe-area-bottom, 0px) + 110px);
  width: 96px;
  height: 140px;
  border-radius: 16px;
  touch-action: none;
  cursor: grab;
}
.ft-call__slot--remote {
  display: grid;
  place-items: center;
}
.ft-call__paused {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: 0;
  padding: 8px 14px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--ft-bg) 70%, transparent);
  color: var(--ft-text);
  font-size: 14px;
}
.ft-call__slot--local .ft-call__paused,
.ft-call__slot--remote.is-thumb .ft-call__paused {
  position: absolute;
  inset: 0;
  justify-content: center;
  padding: 0;
  border-radius: inherit;
  font-size: 28px;
}

.ft-call__notices {
  position: relative;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-2);
  margin-top: auto;
  margin-bottom: var(--ft-space-4);
  max-width: 420px;
}
.ft-call__notice {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 8px 14px;
  border-radius: 16px;
  background: color-mix(in srgb, var(--ft-bg) 75%, transparent);
  backdrop-filter: blur(10px);
  font-size: 14px;
  text-align: start;
}
.ft-call__notice ion-icon {
  flex-shrink: 0;
  font-size: 20px;
}
.ft-call__notice--invite {
  color: var(--ft-accent);
}
.ft-round.is-on {
  background: var(--ft-text);
  color: var(--ft-bg);
}
.ft-call__hangup {
  background: var(--ion-color-danger);
  color: #fff;
  transform: rotate(135deg);
}
</style>

<style>
/* native pictures sit under the WebView, nothing may paint behind the call screen */
html.ft-call-video,
html.ft-call-video body,
html.ft-call-video ion-app {
  background: transparent !important;
}
html.ft-call-video .ft-call {
  --background: transparent;
}
</style>
