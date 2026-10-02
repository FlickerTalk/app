<script setup lang="ts">
import { computed, onUnmounted, ref, watch } from "vue";
import { IonIcon } from "@ionic/vue";
import { callOutline, pauseCircleOutline, videocamOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import { chat } from "../core";
import { call, hangUp } from "../calls";

// 2026-09-29: a call going on while the app shows another screen can always be gone back to and
// hung up. On the iPhone a call answered from CallKit's banner used to leave no way to hang up.
const route = useRoute();
const router = useRouter();
const screen = computed(() => `/call/${call.contact}`);
const shown = computed(
  () => ["calling", "connecting", "active"].includes(call.phase) && Boolean(call.contact) && route.path !== screen.value,
);
const name = computed(() => chat(call.contact)?.name ?? call.contact.slice(0, 9));
// The call as it is now (2026-09-29): with native video either side may turn a camera on or off at
// any moment; out of the call screen my camera is held.
const paused = computed(() => call.native && call.view.camera && call.view.paused);
const icon = computed(() => {
  if (paused.value) return pauseCircleOutline;
  const video = call.native ? call.view.camera || call.view.remote : call.video;
  return video ? videocamOutline : callOutline;
});

// The call's clock, only while the bar is on the screen.
const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;
watch(
  shown,
  (visible) => {
    clearInterval(ticking);
    if (visible) ticking = setInterval(() => (now.value = Date.now()), 1000);
  },
  { immediate: true },
);
onUnmounted(() => clearInterval(ticking));

const pad = (value: number) => String(value).padStart(2, "0");
const clock = computed(() => {
  if (call.phase !== "active") return "";
  const seconds = Math.max(0, Math.floor((now.value - call.since) / 1000));
  return `${pad(Math.floor(seconds / 60))}:${pad(seconds % 60)}`;
});

function back() {
  void router.push(screen.value);
}
</script>

<template>
  <div v-if="shown" class="ft-callbar" data-test="call-bar">
    <button
      type="button"
      class="ft-callbar__back"
      :aria-label="$t('calls.backToCall')"
      :aria-describedby="paused ? 'ft-callbar-paused' : undefined"
      @click="back"
    >
      <ion-icon :icon="icon" aria-hidden="true" />
      <span v-if="paused" id="ft-callbar-paused" class="ft-callbar__paused">{{ $t("calls.cameraPaused") }}</span>
      <span class="ft-callbar__name" dir="auto">{{ name }}</span>
      <span v-if="clock" class="ft-callbar__clock">{{ clock }}</span>
    </button>
    <button type="button" class="ft-round ft-callbar__hangup" :aria-label="$t('calls.hangUp')" @click="hangUp">
      <ion-icon :icon="callOutline" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.ft-callbar {
  position: fixed;
  z-index: 999;
  top: calc(env(safe-area-inset-top) + 6px);
  inset-inline: 0;
  display: flex;
  align-items: center;
  gap: 6px;
  width: fit-content;
  max-width: min(calc(100% - 20px), 360px);
  margin-inline: auto;
  padding: 4px;
  padding-inline-start: 12px;
  border-radius: 999px;
  background: var(--ion-color-success);
  color: #fff;
  box-shadow: 0 10px 24px -12px rgba(0, 0, 0, 0.7);
}
.ft-callbar__back {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  padding: 0;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  font-size: 14px;
  font-weight: 600;
}
.ft-callbar__back ion-icon {
  flex-shrink: 0;
  font-size: 18px;
}
/* Only for screen readers: the icon already shows the pause. */
.ft-callbar__paused {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
}
.ft-callbar__name {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-callbar__clock {
  font-variant-numeric: tabular-nums;
  opacity: 0.85;
}
.ft-callbar .ft-callbar__hangup {
  width: 32px;
  height: 32px;
  font-size: 16px;
  color: #fff;
  background: var(--ion-color-danger);
  transform: rotate(135deg);
}
</style>
