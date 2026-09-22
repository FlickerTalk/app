<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch, watchEffect } from "vue";
import { IonContent, IonIcon, IonPage } from "@ionic/vue";
import { callOutline, micOffOutline, micOutline, videocamOffOutline, videocamOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import { chat } from "../core";
import { call, hangUp, startCall, toggleCamera, toggleMute } from "../calls";
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
});
onUnmounted(() => {
  clearInterval(ticking);
  // Leaving the screen ends the call: no call goes on out of sight.
  if (call.phase !== "idle" && call.phase !== "ended") void hangUp();
});
watch(
  () => call.phase,
  (phase) => {
    if (phase === "ended") setTimeout(() => router.back(), 1500);
  },
);
</script>

<template>
  <ion-page>
    <ion-content class="ft-call">
      <div class="ft-call__body" :class="{ 'is-video': isVideo, 'is-live': Boolean(call.remote) }">
        <div v-if="isVideo" class="ft-call__video" data-test="video">
          <video v-if="call.remote" ref="remoteVideo" class="ft-call__remote" autoplay playsinline />
          <video v-show="call.local" ref="localVideo" class="ft-call__self" autoplay playsinline muted />
        </div>
        <audio v-else ref="remoteAudio" autoplay />

        <div class="ft-call__peer">
          <Avatar v-if="!isVideo || !call.remote" :name="contact.name" :hue="contact.hue" :size="132" />
          <h1 class="ft-call__name">{{ contact.name }}</h1>
          <span class="ft-call__state" :class="{ 'is-live': call.phase === 'active' }">{{ state }}</span>
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
            <ion-icon :icon="call.muted ? micOffOutline : micOutline" aria-hidden="true" />
          </button>
          <button
            v-if="isVideo"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': call.cameraOff }"
            :aria-label="$t('calls.camera')"
            :aria-pressed="!call.cameraOff"
            @click="toggleCamera"
          >
            <ion-icon :icon="call.cameraOff ? videocamOffOutline : videocamOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-call__hangup" :aria-label="$t('calls.hangUp')" @click="hangUp">
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
  right: var(--ft-space-4);
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
  gap: var(--ft-space-4);
}
.ft-call__controls .ft-round {
  width: 58px;
  height: 58px;
  font-size: 26px;
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
