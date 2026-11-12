<script setup lang="ts">
import { IonIcon } from "@ionic/vue";
import {
  callOutline,
  cameraReverseOutline,
  headsetOutline,
  micOffOutline,
  micOutline,
  phonePortraitOutline,
  videocamOffOutline,
  videocamOutline,
  volumeHighOutline,
} from "ionicons/icons";
import { call, toggleCamera, toggleMute, toggleSpeaker } from "../calls";

defineProps<{
  native: boolean;
  isVideo: boolean;
  invite: boolean;
  cameraUsable: boolean;
  cameraRunning: boolean;
}>();
const emit = defineEmits<{ camera: []; flip: []; hangUp: [] }>();
</script>

<template>
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
      @click="emit('camera')"
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
      @click="emit('flip')"
    >
      <ion-icon :icon="cameraReverseOutline" aria-hidden="true" />
    </button>
    <button type="button" class="ft-round ft-call__hangup" :aria-label="$t('calls.hangUp')" @click="emit('hangUp')">
      <ion-icon :icon="callOutline" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
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
