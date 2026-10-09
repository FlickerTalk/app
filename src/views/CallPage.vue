<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch, watchEffect } from "vue";
import { IonActionSheet, IonButton, IonContent, IonIcon, IonPage, IonSpinner, onIonViewDidEnter, onIonViewWillLeave, toastController } from "@ionic/vue";
import {
  callOutline,
  cameraReverseOutline,
  chevronDown,
  documentOutline,
  downloadOutline,
  easelOutline,
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
import { PRESENT_BOARD, PRESENT_DOCUMENT, PRESENT_FILE_LIMIT, SEND_WAIT, canPresentWith, needsPresentGrant, presentGrant, sentFile, waitFor } from "../present";
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

// Plan §66: calls are always peer to peer; the relay only steps in per the user's setting (§17).
const id = computed(() => String(route.params.id));
const current = computed(() => call.contact === id.value && call.phase !== "idle");
const contact = computed(() => chat(id.value) ?? { name: "", hue: 0 });
const isVideo = computed(() => (current.value ? call.video : Boolean(route.query.video)));

const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;

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

// ---- Presenting (docs/plan-presentar-en-llamada.md, 2026-10-08) ----
// A whiteboard or a PDF fills the call screen on both phones; the core says who presents
// (`call.presenting`), the app opens the tool. A tool talks to its twin only with `live`, asked
// once with the games' sheet.
const presenting = computed(() => (current.value && call.phase === "active" ? call.presenting : null));
// The tool stays as it is while this key does: a lost link says the same presentation again.
const presentKey = computed(() => (presenting.value ? `${presenting.value.by}|${presenting.value.plugin}|${presenting.value.file ?? ""}` : ""));
const presentPlugin = computed(() => installed.value.find((one) => one.id === presenting.value?.plugin));
/** Present appears only when the other phone can see it (`media` ≥ 2) and nobody presents. The
 *  state comes only from the core's events: `presentInCall` itself changes nothing here. */
const canOffer = computed(() => current.value && call.native && call.canPresent && call.phase === "active" && !call.presenting);
const choosing = ref(false);
/** A presentation on its way (a PDF can take a while to reach the chat): Present waits for it. */
const starting = ref(false);
/** Runs one start at a time; a second tap while one is on its way does nothing. */
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
/** The tool whose permission sheet is open. */
const asking = ref<Asking | null>(null);
/** The presentation of theirs the user said no to: not asked again. */
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
  await refreshPlugins();
  const now = installed.value.find((one) => one.id === tool);
  return Boolean(now && !needsPresentGrant(now));
}

/** Present, from the sheet: the tool must be here, unlocked and allowed to talk to its twin. */
function present(tool: string) {
  choosing.value = false;
  return once(async () => {
    await Promise.all([refreshPlugins(), refreshPremiumLock()]);
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
          if (await grantLive(tool)) await begin(tool);
        }),
    };
  });
}

/** The board presents at once; a PDF first goes to the chat as a file, then that message is shown. */
async function begin(tool: string) {
  try {
    if (tool !== PRESENT_DOCUMENT) return await presentInCall(call.id, tool);
    const [file] = await pickFiles("application/pdf");
    if (!file) return;
    if (file.size > PRESENT_FILE_LIMIT) return await say("calls.presentTooBig");
    await loadMessages(id.value);
    const before = new Set((chat(id.value)?.messages ?? []).map((one) => one.id));
    await sendPicked(id.value, file);
    const message = await waitFor(() => sentFile(chat(id.value)?.messages ?? [], before, file.name), SEND_WAIT);
    if (!message) throw new Error("the PDF did not show up in the chat");
    await presentInCall(call.id, PRESENT_DOCUMENT, message);
  } catch (error) {
    // The plan can change after the check above: the core's refusal has the last word.
    if (needsSubscription(error)) return void router.push(PREMIUM_PAGE);
    // The core said their app is too old (plan A); anything else is a plain failure (§84).
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

type PresentState = "none" | "missing" | "ask" | "declined" | "accept" | "loading" | "failed" | "sheet";
const fileMessage = computed(() => {
  const file = presenting.value?.file;
  return file ? chat(id.value)?.messages.find((one) => one.id === file) : undefined;
});
const presentFile = ref<HandedFile | undefined>();
const presentBroken = ref(false);
const presentState = computed<PresentState>(() => {
  const now = presenting.value;
  if (!now) return "none";
  const plugin = presentPlugin.value;
  if (!canPresentWith(plugin)) return "missing";
  // No lock here (Ioan, 2026-10-08): following is free, the core decides; presenting checks it first.
  if (needsPresentGrant(plugin)) return declined.value === presentKey.value ? "declined" : "ask";
  if (presentBroken.value) return "failed";
  if (!now.file || presentFile.value) return "sheet";
  if (now.by === "me") return "loading";
  const arrived = fileMessage.value?.file?.state;
  if (arrived === "failed") return "failed";
  return arrived === "waiting" ? "accept" : "loading";
});
/** The presentation takes the screen (the tool, or what it waits for); a refusal or a lack does not. */
const presentArea = computed(() => ["sheet", "accept", "loading", "failed"].includes(presentState.value));
/** The hole the presentation leaves for their picture (Task 12 draws it). */
const presentClip = ref<string | undefined>();

// Their presentation came and the tool may not talk to its twin yet: ask, once.
watch(
  presentState,
  (state) => {
    const plugin = presentPlugin.value;
    if (state === "ask" && plugin && presenting.value?.by === "them" && !asking.value) {
      asking.value = {
        them: true,
        name: pluginName(plugin),
        body: t("calls.presentAsk", { name: contact.value.name, plugin: pluginName(plugin) }),
        icon: pluginIcon(plugin),
        image: pluginImage(plugin),
        allow: async () => void (await grantLive(plugin.id)),
      };
    }
    if (state !== "ask" && asking.value?.them) asking.value = null;
  },
  { immediate: true },
);

// A new presentation: what the last one loaded goes, the plugins and the chat are read again.
watch(
  presentKey,
  (key) => {
    presentFile.value = undefined;
    presentBroken.value = false;
    if (!key) return;
    void refreshPlugins();
    if (presenting.value?.file) void loadMessages(id.value).catch(() => undefined);
  },
  { immediate: true },
);
// The PDF opens once it is here whole: mine at once, theirs when the chat says it arrived and the
// core says it is a file of this chat (its message may have come after the presentation did).
watch(
  [presentKey, presentState, () => fileMessage.value?.file?.state],
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

function acceptPresented() {
  const file = presenting.value?.file;
  if (file) void acceptFile(file).catch(() => undefined);
}

/** The tool asked to close: the presenter stops; a follower's goes when the presenter stops. */
function presentDone() {
  if (presenting.value?.by === "me") void stopPresenting(call.id);
}

// Native video (2026-09-29, docs/video-nativo.md): on the phones the pictures are native views
// under the WebView. This screen leaves see-through holes where they go and tells the core where
// those are; the controls stay HTML, over the pictures. Each side owns its camera: the switch is
// there at all times, and theirs coming on only invites me to turn mine on.
const native = computed(() => current.value && call.native);
const live = computed(() => call.phase === "active");
const cameraReady = computed(() => live.value && call.view.available);
// My camera runs only once the core has the video line up: before that there is nothing under a
// see-through page but the window's white (found by QA, 2026-09-29).
const cameraRunning = computed(() => cameraReady.value && call.view.camera);
const stage = computed(() => native.value && live.value && (cameraRunning.value || call.view.remote));
const showRemote = computed(() => stage.value && call.view.remote);
const showLocal = computed(() => stage.value && cameraRunning.value);
const invite = computed(() => native.value && cameraReady.value && call.view.remote && !call.view.camera);
// A video call with no pictures to show sits on a dark call background, not the page's (white in
// the light theme); see-through, the native views paint it black.
const dark = computed(() => native.value && !stage.value && (call.video || call.view.camera || call.view.remote));
// The camera button can always turn my camera off.
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

// Always on a phone (2026-09-29), but it only flips a camera that runs.
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
    // A thumbnail over their picture has round corners; mine alone fills the screen.
    localRadius: showRemote.value ? 16 : 0,
  };
}
/** On this screen: the core is told where the pictures go (nowhere, on a voice call). */
let shown = true;
/** On this screen, for Android's back button (Ionic keeps a page mounted under the next one). */
const onScreen = ref(true);
// Android's back button closes the presenting sheets first, the last one opened first.
closeOnBackWhile(() => onScreen.value && choosing.value, () => (choosing.value = false));
closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => answerAsk(false));
function relayout() {
  if (shown && native.value) layoutVideo(measure);
}
function hide() {
  shown = false;
  if (native.value) hideVideo();
}

// While the pictures show, the app is see-through around them (the class lives on `html`).
const seeThrough = ref(true);
watchEffect(() => document.documentElement.classList.toggle("ft-call-video", Boolean(stage.value) && seeThrough.value));
// A dark screen whatever the appearance: the system bars' icons turn light over it (2026-10-02).
watchEffect(() => darkScreen("call", onScreen.value && Boolean(stage.value || dark.value)));

watch([native, stage, showRemote, showLocal, () => call.view.facing], relayout, { flush: "post" });
const sized = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(relayout);
watch([body, remoteSlot, localSlot], (elements) => {
  sized?.disconnect();
  for (const element of elements) if (element) sized?.observe(element);
});
onIonViewDidEnter(() => {
  shown = true;
  onScreen.value = true;
  seeThrough.value = true;
  relayout();
});
onIonViewWillLeave(() => {
  onScreen.value = false;
  clearTimeout(leaving);
  hide();
  seeThrough.value = false;
});

// My thumbnail moves where the thumb drags it, and stays inside the screen.
const drag = reactive({ x: 0, y: 0 });
let grab: { id: number; x: number; y: number; fromX: number; fromY: number } | null = null;
const thumbStyle = computed(() => (showRemote.value && (drag.x || drag.y) ? { transform: `translate(${drag.x}px, ${drag.y}px)` } : undefined));
watch(showRemote, () => Object.assign(drag, { x: 0, y: 0 }));
function grabThumb(event: PointerEvent) {
  if (!showRemote.value) return;
  grab = { id: event.pointerId, x: event.clientX, y: event.clientY, fromX: drag.x, fromY: drag.y };
  (event.currentTarget as Element | null)?.setPointerCapture?.(event.pointerId);
}
function moveThumb(event: PointerEvent) {
  if (!grab || event.pointerId !== grab.id) return;
  drag.x = grab.fromX + event.clientX - grab.x;
  drag.y = grab.fromY + event.clientY - grab.y;
  relayout();
}
function dropThumb(event: PointerEvent) {
  if (!grab || event.pointerId !== grab.id) return;
  grab = null;
  const box = localSlot.value?.getBoundingClientRect();
  const area = body.value?.getBoundingClientRect();
  if (box && area) {
    if (box.left < area.left) drag.x += area.left - box.left;
    if (box.right > area.right) drag.x -= box.right - area.right;
    if (box.top < area.top) drag.y += area.top - box.top;
    if (box.bottom > area.bottom) drag.y -= box.bottom - area.bottom;
  }
  relayout();
}

const remoteVideo = ref<HTMLVideoElement | null>(null);
const localVideo = ref<HTMLVideoElement | null>(null);
const remoteAudio = ref<HTMLAudioElement | null>(null);
watchEffect(() => {
  if (remoteVideo.value) remoteVideo.value.srcObject = call.remote;
  if (localVideo.value) localVideo.value.srcObject = call.local;
  if (remoteAudio.value) remoteAudio.value.srcObject = call.remote;
});

// On the page until Ionic takes it away, after its leaving transition: the call bar waits for that.
// Under pages opened over it, it stays in the history, and going back to the call comes back here.
let unmounted: (() => void) | undefined;
onMounted(() => {
  unmounted = callScreenMounted(router);
  void refreshPlugins();
  void refreshPremiumLock();
  if (!current.value || call.phase === "ended") void startCall(id.value, Boolean(route.query.video));
  ticking = setInterval(() => (now.value = Date.now()), 1000);
  window.addEventListener("resize", relayout);
  window.addEventListener("orientationchange", relayout);
  relayout();
});
// Out of the call screen, once (2026-09-28): back where the call came from or, with nothing behind
// (opened from a notification, or reloaded), to the conversation. Hanging up always leaves, even
// when the call already ended on its own ("Unreachable"): the screen used to stay there.
// Leaving keeps the call (2026-09-29): the call bar shows it and my camera is held; only the
// hang-up button ends it. Android's back button leaves the same way, never out of the app, and
// so does the on-screen button at the top (2026-10-03): tablets and iPhones have no back button.
let left = false;
let leaving: ReturnType<typeof setTimeout> | undefined;
// It is over once it got there, as a tool's own page (`closeAll`, 2026-10-08).
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
    // Only from this screen: the call may end while another one is in front.
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
        <ion-action-sheet :is-open="choosing" :header="$t('calls.present')" :buttons="presentButtons" @did-dismiss="choosing = false" />
        <p v-if="presenting?.by === 'them'" class="ft-call__presenter" data-test="presenter" dir="auto">
          {{ $t("calls.presenting", { name: contact.name }) }}
        </p>
        <section
          v-if="presentArea"
          class="ft-call__present"
          :style="presentClip ? { clipPath: presentClip } : undefined"
          data-test="present-area"
        >
          <PluginSheet
            v-if="presentState === 'sheet' && presentPlugin && presenting"
            :key="presentKey"
            :plugin="{ id: presentPlugin.id, name: pluginName(presentPlugin) }"
            :contact="id"
            :live="true"
            :file="presentFile"
            :session="sessionOf(id)"
            :presenting="presenting.by === 'me' ? 'lead' : 'follow'"
            @done="presentDone"
            @refused="presentBroken = true"
          />
          <div v-else class="ft-call__present-wait">
            <ion-button v-if="presentState === 'accept'" shape="round" data-test="present-accept" @click="acceptPresented">
              <ion-icon slot="start" :icon="downloadOutline" aria-hidden="true" />
              {{ $t("calls.acceptToSee") }}
            </ion-button>
            <p v-else-if="presentState === 'failed'" class="ft-call__notice" role="alert" data-test="present-failed">{{ $t("calls.presentFailed") }}</p>
            <ion-spinner v-else name="crescent" color="medium" data-test="present-loading" aria-hidden="true" />
          </div>
        </section>
        <div v-if="native" class="ft-call__stage" :data-test="stage ? 'video' : undefined">
          <div v-if="showRemote" ref="remoteSlot" class="ft-call__slot ft-call__slot--remote" data-test="remote-slot">
            <p v-if="call.view.remotePaused" class="ft-call__paused" data-test="remote-paused">
              <ion-icon :icon="pauseCircleOutline" aria-hidden="true" />
              {{ $t("calls.cameraPaused") }}
            </p>
          </div>
          <div
            v-if="showLocal"
            ref="localSlot"
            class="ft-call__slot ft-call__slot--local"
            :class="{ 'is-thumb': showRemote }"
            :style="thumbStyle"
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

        <!-- Hidden, not taken out: the notices and the controls keep their place under the presentation. -->
        <div class="ft-call__peer" :style="presentArea ? { visibility: 'hidden' } : undefined">
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
          <p v-if="invite" class="ft-call__notice ft-call__notice--invite">
            <ion-icon :icon="videocamOutline" aria-hidden="true" />
            {{ $t("calls.turnOnCamera") }}
          </p>
          <p v-if="presentState === 'missing'" class="ft-call__notice" role="status" data-test="present-missing">
            <ion-icon :icon="easelOutline" aria-hidden="true" />
            {{ $t("calls.presentMissing") }}
          </p>
        </div>

        <div class="ft-call__controls">
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
              :key="call.speaker ? 'speaker-on' : 'speaker-off'"
              :icon="call.speaker ? volumeHighOutline : phonePortraitOutline"
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
/* A video call without pictures: a dark call background whatever the theme, with its own ink. */
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

/* Leaves the screen and keeps the call, like Android's back button: top corner, inline start. */
.ft-call__minimize {
  position: absolute;
  z-index: 1;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline-start: calc(max(env(safe-area-inset-left), env(safe-area-inset-right)) + var(--ft-space-2));
  /* Apple's 44 pt tap target. */
  min-width: 44px;
  min-height: 44px;
  /* Ionic's medium, not the color prop: a colored clear button drops its background, and the
     button needs one to show over the pictures. */
  --color: var(--ion-color-medium);
  --background: rgba(var(--ion-color-medium-rgb), 0.18);
}
/* Present, or stop presenting: the top corner opposite the way back, the same 44 pt target. */
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
/* Who presents, between the two corner buttons. */
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
/* The presentation: from under the top buttons to over the controls; absolute, so nothing moves. */
.ft-call__present {
  position: absolute;
  z-index: 1;
  inset-inline: 0;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2) * 2 + 44px);
  bottom: calc(var(--ion-safe-area-bottom, 0px) + var(--ft-space-5) * 2 + 58px);
  overflow-y: auto;
  background: var(--ion-background-color);
}
/* What the presentation waits for floats in its middle. */
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
/* The whole picture, whatever its shape: never cropped (a tablet's camera on a phone, say). */
.ft-call__remote {
  width: 100%;
  height: 100%;
  background: #000;
  object-fit: contain;
}
/* The front camera, as a mirror. */
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
/* Five buttons still fit a narrow phone. */
.ft-call__controls .ft-round {
  width: clamp(48px, 14vw, 58px);
  height: clamp(48px, 14vw, 58px);
  font-size: 26px;
}
.ft-round.is-waiting {
  opacity: 0.45;
}
/* Their camera came on: mine is offered, never turned on for me. */
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

/* Native pictures: holes the native views show through (docs/video-nativo.md). */
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
.ft-call__slot--local.is-thumb {
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
.ft-call__slot--local .ft-call__paused {
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
/* Native pictures sit under the WebView: while they show, nothing of the app paints behind the
   call screen (the pages under it are hidden by Ionic). */
html.ft-call-video,
html.ft-call-video body,
html.ft-call-video ion-app {
  background: transparent !important;
}
html.ft-call-video .ft-call {
  --background: transparent;
}
</style>
