<script setup lang="ts">
import { computed } from "vue";
import { IonIcon, IonRippleEffect } from "@ionic/vue";
import { chevronBackOutline, chevronForwardOutline } from "ionicons/icons";
import { t } from "../i18n";

const props = defineProps<{ collapsed: boolean; controls: string }>();
const emit = defineEmits<{ toggle: [] }>();

const label = computed(() => t(props.collapsed ? "chats.expandList" : "chats.collapseList"));
</script>

<template>
  <!-- ion-button copies aria attributes only when it loads, so a label that changes needs a native button -->
  <button
    type="button"
    class="ft-list-toggle ion-activatable"
    data-test="list-toggle"
    :aria-label="label"
    :title="label"
    :aria-expanded="!collapsed"
    :aria-controls="controls"
    @click="emit('toggle')"
  >
    <ion-icon :icon="collapsed ? chevronForwardOutline : chevronBackOutline" flip-rtl aria-hidden="true" />
    <ion-ripple-effect />
  </button>
</template>

<style scoped>
.ft-list-toggle {
  appearance: none;
  position: relative;
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  padding: 0;
  overflow: hidden;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  font-size: 20px;
  cursor: pointer;
}
@media (hover: hover) {
  .ft-list-toggle:hover {
    background: color-mix(in srgb, var(--ft-surface-2) 60%, transparent);
  }
}
.ft-list-toggle:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}
</style>
