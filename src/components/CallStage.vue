<script setup lang="ts">
import { ref } from "vue";
import { IonIcon } from "@ionic/vue";
import { pauseCircleOutline } from "ionicons/icons";
import { call } from "../calls";

defineProps<{
  native: boolean;
  isVideo: boolean;
  stage: boolean;
  showRemote: boolean;
  showLocal: boolean;
  remoteThumb: boolean;
  localThumb: boolean;
  thumbStyle?: { transform: string };
}>();
const emit = defineEmits<{ grab: [event: PointerEvent]; move: [event: PointerEvent]; letGo: [event: PointerEvent] }>();

const remoteSlot = ref<HTMLElement | null>(null);
const localSlot = ref<HTMLElement | null>(null);
const remoteVideo = ref<HTMLVideoElement | null>(null);
const localVideo = ref<HTMLVideoElement | null>(null);
const remoteAudio = ref<HTMLAudioElement | null>(null);
defineExpose({ remoteSlot, localSlot, remoteVideo, localVideo, remoteAudio });
</script>

<template>
  <div v-if="native" class="ft-call__stage" :data-test="stage ? 'video' : undefined">
    <div
      v-if="showRemote"
      ref="remoteSlot"
      class="ft-call__slot ft-call__slot--remote"
      :class="{ 'is-thumb': remoteThumb }"
      :style="remoteThumb ? thumbStyle : undefined"
      data-test="remote-slot"
      @pointerdown="emit('grab', $event)"
      @pointermove="emit('move', $event)"
      @pointerup="emit('letGo', $event)"
      @pointercancel="emit('letGo', $event)"
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
      @pointerdown="emit('grab', $event)"
      @pointermove="emit('move', $event)"
      @pointerup="emit('letGo', $event)"
      @pointercancel="emit('letGo', $event)"
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
</template>

<style scoped>
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
</style>
