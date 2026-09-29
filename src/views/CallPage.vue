<script setup lang="ts">
import { computed, onMounted, onUnmounted, reactive, ref, watch, watchEffect } from "vue";
import { IonContent, IonIcon, IonPage, onIonViewDidEnter, onIonViewWillLeave } from "@ionic/vue";
import {
  callOutline,
  cameraReverseOutline,
  micOffOutline,
  micOutline,
  pauseCircleOutline,
  phonePortraitOutline,
  videocamOffOutline,
  videocamOutline,
  volumeHighOutline,
} from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import { chat } from "../core";
import {
  call,
  hangUp,
  hideVideo,
  layoutVideo,
  rectOf,
  startCall,
  switchCamera,
  toggleCamera,
  toggleMute,
  toggleSpeaker,
  type VideoLayout,
} from "../calls";
import { t } from "../i18n";

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

// Native video (2026-09-29, docs/video-nativo.md): on the phones the pictures are native views
// under the WebView. This screen leaves see-through holes where they go and tells the core where
// those are; the controls stay HTML, over the pictures. Each side owns its camera: the switch is
// there at all times, and theirs coming on only invites me to turn mine on.
const native = computed(() => current.value && call.native);
const live = computed(() => call.phase === "active");
const stage = computed(() => native.value && live.value && (call.view.camera || call.view.remote));
const showRemote = computed(() => stage.value && call.view.remote);
const showLocal = computed(() => stage.value && call.view.camera);
const cameraReady = computed(() => live.value && call.view.available);
const invite = computed(() => native.value && cameraReady.value && call.view.remote && !call.view.camera);

const unavailable = ref(false);
let unavailableTimer: ReturnType<typeof setTimeout> | undefined;
function camera() {
  if (!native.value || cameraReady.value) {
    void toggleCamera();
    return;
  }
  if (!live.value) return;
  unavailable.value = true;
  clearTimeout(unavailableTimer);
  unavailableTimer = setTimeout(() => (unavailable.value = false), 4000);
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

watch([native, stage, showRemote, showLocal, () => call.view.facing], relayout, { flush: "post" });
const sized = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(relayout);
watch([body, remoteSlot, localSlot], (elements) => {
  sized?.disconnect();
  for (const element of elements) if (element) sized?.observe(element);
});
onIonViewDidEnter(() => {
  shown = true;
  seeThrough.value = true;
  relayout();
});
onIonViewWillLeave(() => {
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

onMounted(() => {
  if (!current.value || call.phase === "ended") void startCall(id.value, Boolean(route.query.video));
  ticking = setInterval(() => (now.value = Date.now()), 1000);
  window.addEventListener("resize", relayout);
  window.addEventListener("orientationchange", relayout);
  relayout();
});
// Out of the call screen, once (2026-09-28): back where the call came from or, with nothing behind
// (opened from a notification, or reloaded), to the conversation. Hanging up always leaves, even
// when the call already ended on its own ("Unreachable"): the screen used to stay there.
let left = false;
let leaving: ReturnType<typeof setTimeout> | undefined;
function leave() {
  if (left) return;
  left = true;
  clearTimeout(leaving);
  if (window.history.state?.back) router.back();
  else void router.replace(`/chat/${id.value}`);
}
function end() {
  if (call.phase !== "idle" && call.phase !== "ended") void hangUp();
  leave();
}

onUnmounted(() => {
  clearInterval(ticking);
  clearTimeout(unavailableTimer);
  window.removeEventListener("resize", relayout);
  window.removeEventListener("orientationchange", relayout);
  sized?.disconnect();
  hide();
  seeThrough.value = false;
  document.documentElement.classList.remove("ft-call-video");
  left = true;
  clearTimeout(leaving);
  // Leaving the screen ends the call: no call goes on out of sight.
  if (call.phase !== "idle" && call.phase !== "ended") void hangUp();
});
watch(
  () => call.phase,
  (phase) => {
    if (phase === "ended") leaving = setTimeout(leave, 1500);
  },
);
</script>

<template>
  <ion-page>
    <ion-content class="ft-call">
      <div
        ref="body"
        class="ft-call__body"
        :class="native ? { 'is-video': stage, 'is-live': stage } : { 'is-video': isVideo, 'is-live': Boolean(call.remote) }"
      >
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

        <div class="ft-call__peer">
          <Avatar v-if="native ? !stage : !isVideo || !call.remote" :name="contact.name" :hue="contact.hue" :size="132" />
          <h1 class="ft-call__name">{{ contact.name }}</h1>
          <span class="ft-call__state" :class="{ 'is-live': call.phase === 'active' }">{{ state }}</span>
        </div>

        <div class="ft-call__notices">
          <p v-if="call.cameraDenied" class="ft-call__notice" role="alert" data-test="camera-denied">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.cameraDenied") }}
          </p>
          <p v-if="unavailable" class="ft-call__notice" role="status">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.videoUnavailable") }}
          </p>
          <p v-if="invite" class="ft-call__notice ft-call__notice--invite">
            <ion-icon :icon="videocamOutline" aria-hidden="true" />
            {{ $t("calls.turnOnCamera") }}
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
            :class="{ 'is-on': call.view.camera, 'is-invite': invite, 'is-waiting': !cameraReady }"
            :aria-label="$t('calls.camera')"
            :aria-pressed="call.view.camera"
            :aria-disabled="!cameraReady"
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
            v-if="native && call.view.camera && cameraReady"
            type="button"
            class="ft-round ft-round--ghost"
            :aria-label="$t('calls.switchCamera')"
            @click="switchCamera"
          >
            <ion-icon :icon="cameraReverseOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-call__hangup" :aria-label="$t('calls.hangUp')" @click="end">
            <ion-icon :icon="callOutline" aria-hidden="true" />
          </button>
        </div>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-call {
  --background: var(--ft-bg);
}
.ft-call__body {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  min-height: 100%;
  padding: calc(env(safe-area-inset-top) + var(--ft-space-5)) var(--ft-space-4)
    calc(env(safe-area-inset-bottom) + var(--ft-space-5));
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
  bottom: calc(env(safe-area-inset-bottom) + 110px);
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
  bottom: calc(env(safe-area-inset-bottom) + 110px);
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
