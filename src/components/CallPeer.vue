<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref } from "vue";
import Avatar from "./Avatar.vue";
import { call } from "../calls";
import { t } from "../i18n";

defineProps<{ name: string; hue: number; avatar: boolean }>();

const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;
onMounted(() => (ticking = setInterval(() => (now.value = Date.now()), 1000)));
onUnmounted(() => clearInterval(ticking));

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
</script>

<template>
  <div class="ft-call__peer">
    <Avatar v-if="avatar" :name="name" :hue="hue" :size="132" />
    <h1 class="ft-call__name" dir="auto">{{ name }}</h1>
    <span class="ft-call__state" :class="{ 'is-live': call.phase === 'active' }">{{ state }}</span>
  </div>
</template>

<style scoped>
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
</style>
