<script setup lang="ts">
import { computed } from "vue";

const props = withDefaults(
  defineProps<{ name: string; hue: number; size?: number; connected?: boolean }>(),
  { size: 48, connected: false },
);

const initials = computed(() =>
  props.name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((part) => part[0].toUpperCase())
    .join(""),
);

const style = computed(() => ({
  width: `${props.size}px`,
  height: `${props.size}px`,
  fontSize: `${Math.round(props.size * 0.36)}px`,
  background: `linear-gradient(135deg, hsl(${props.hue} 72% 62%), hsl(${(props.hue + 40) % 360} 66% 46%))`,
}));
</script>

<template>
  <span class="ft-avatar" :style="style">
    {{ initials }}
    <span v-if="connected" class="ft-avatar__spark ft-spark" role="img" :aria-label="$t('status.connected')" />
  </span>
</template>

<style scoped>
.ft-avatar {
  position: relative;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  border-radius: 50%;
  color: #fff;
  font-weight: 600;
  letter-spacing: 0.02em;
  text-shadow: 0 1px 1px rgba(0, 0, 0, 0.15);
  user-select: none;
}

.ft-avatar__spark {
  position: absolute;
  inset-inline-end: 0;
  bottom: 0;
}
</style>
