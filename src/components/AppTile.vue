<script setup lang="ts">
import { computed, onBeforeUnmount } from "vue";
import { IonIcon, IonRippleEffect, IonSpinner } from "@ionic/vue";
import { addOutline, downloadOutline, lockClosedOutline } from "ionicons/icons";
import { t } from "../i18n";

const props = defineProps<{
  name: string;
  icon: string;
  image?: string;
  badge?: "download" | "add" | "lock";
  off?: boolean;
  caption?: string;
  busy?: boolean;
  label?: string;
}>();
const emit = defineEmits<{ tap: []; hold: [] }>();

const HOLD_MS = 500;
const SLOP_PX = 10;

const BADGES = { download: downloadOutline, add: addOutline, lock: lockClosedOutline } as const;

const ariaLabel = computed(() => {
  if (props.label) return props.label;
  switch (props.badge) {
    case "lock":
      return t("apps.lockedLabel", { name: props.name });
    case "download":
      return t("apps.downloadLabel", { name: props.name, size: props.caption ?? "" }).trim();
    case "add":
      return t("apps.addLabel", { name: props.name });
    default:
      return props.name;
  }
});

let timer: ReturnType<typeof setTimeout> | undefined;
let start = { x: 0, y: 0 };
let held = false;

function cancel() {
  if (timer !== undefined) clearTimeout(timer);
  timer = undefined;
}

function hold() {
  cancel();
  held = true;
  emit("hold");
}

function down(event: PointerEvent) {
  cancel();
  held = false;
  if (event.button !== 0 || props.busy) return;
  start = { x: event.clientX, y: event.clientY };
  timer = setTimeout(hold, HOLD_MS);
}

function move(event: PointerEvent) {
  if (timer === undefined) return;
  if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > SLOP_PX) cancel();
}

function click() {
  if (props.busy) return;
  if (held) {
    held = false;
    return;
  }
  emit("tap");
}

// Android opens the context menu on a long press, treat it as the hold
function menu(event: Event) {
  event.preventDefault();
  if (held || props.busy) return;
  if (timer !== undefined) return hold();
  emit("hold");
}

function key(event: KeyboardEvent) {
  if (event.key !== "Enter" || !event.shiftKey || props.busy) return;
  event.preventDefault();
  emit("hold");
}

onBeforeUnmount(cancel);
</script>

<template>
  <button
    type="button"
    class="ft-app-tile"
    :class="{ 'ft-app-tile--off': off, 'ion-activatable': !busy }"
    :aria-label="ariaLabel"
    :aria-busy="busy ? 'true' : undefined"
    :aria-disabled="busy ? 'true' : undefined"
    @pointerdown="down"
    @pointermove="move"
    @pointerup="cancel"
    @pointercancel="cancel"
    @pointerleave="cancel"
    @click="click"
    @contextmenu="menu"
    @keydown="key"
  >
    <span class="ft-app-tile__icon" :class="{ 'ft-app-tile__icon--image': image }">
      <img v-if="image" class="ft-app-tile__image" :src="image" alt="" draggable="false" />
      <ion-icon v-else :icon="icon" aria-hidden="true" />
      <ion-spinner v-if="busy" name="crescent" class="ft-app-tile__spinner" aria-hidden="true" />
      <span
        v-if="badge"
        class="ft-app-tile__badge"
        :class="`ft-app-tile__badge--${badge}`"
        :data-test="badge === 'lock' ? 'locked' : undefined"
        aria-hidden="true"
      >
        <ion-icon :icon="BADGES[badge]" />
      </span>
    </span>
    <span class="ft-app-tile__name" dir="auto">{{ name }}</span>
    <span v-if="caption" class="ft-app-tile__caption">{{ caption }}</span>
    <!-- a press that started a ripple stays lit until the install ends -->
    <ion-ripple-effect v-if="!busy" />
  </button>
</template>

<style scoped>
.ft-app-tile {
  appearance: none;
  position: relative;
  overflow: hidden;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  min-width: 0;
  padding: 6px 0 4px;
  border: 0;
  border-radius: 14px;
  background: transparent;
  color: var(--ion-text-color);
  font: inherit;
  cursor: pointer;
  /* no text selection and no iOS callout on a long press */
  -webkit-user-select: none;
  user-select: none;
  -webkit-touch-callout: none;
  -webkit-tap-highlight-color: transparent;
}
.ft-app-tile[aria-busy="true"] {
  cursor: progress;
}
.ft-app-tile:focus-visible {
  outline: 2px solid var(--ion-color-primary);
  outline-offset: 2px;
}
.ft-app-tile__icon {
  position: relative;
  display: grid;
  place-items: center;
  width: 64px;
  height: 64px;
  border-radius: 18px;
  background: rgba(var(--ion-color-primary-rgb), 0.16);
  color: var(--ion-color-primary);
}
.ft-app-tile__icon > ion-icon {
  font-size: 30px;
}
.ft-app-tile__spinner {
  position: absolute;
  inset: 0;
  margin: auto;
  width: 40px;
  height: 40px;
  color: var(--ion-color-primary);
}
.ft-app-tile[aria-busy="true"] .ft-app-tile__icon > ion-icon {
  opacity: 0.25;
}
.ft-app-tile .ft-app-tile__icon--image {
  background: transparent;
}
.ft-app-tile__image {
  display: block;
  width: 100%;
  height: 100%;
  border-radius: inherit;
  object-fit: cover;
  -webkit-user-drag: none;
}
.ft-app-tile--off .ft-app-tile__image {
  opacity: 0.55;
}
.ft-app-tile[aria-busy="true"] .ft-app-tile__image {
  opacity: 0.25;
}
.ft-app-tile__name {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  overflow: hidden;
  box-sizing: border-box;
  max-width: 100%;
  /* splitting at any letter gave "Backgammo/n" */
  padding-inline: 0;
  font-size: 12px;
  line-height: 1.25;
  text-align: center;
  overflow-wrap: break-word;
}
.ft-app-tile__caption {
  display: block;
  margin-top: -4px;
  font-size: 11px;
  line-height: 1.2;
  color: var(--ion-color-medium);
}
.ft-app-tile--off .ft-app-tile__icon {
  background: rgba(var(--ion-color-medium-rgb), 0.14);
  color: var(--ion-color-medium);
}
.ft-app-tile--off .ft-app-tile__name {
  opacity: 0.8;
}
.ft-app-tile__badge {
  position: absolute;
  inset-block-start: -6px;
  inset-inline-end: -6px;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border-radius: 50%;
  background: var(--ion-color-primary);
  color: var(--ion-color-primary-contrast);
  box-shadow: 0 0 0 2px var(--ion-background-color);
}
.ft-app-tile__badge ion-icon {
  font-size: 13px;
}
.ft-app-tile__badge--lock {
  background: var(--ion-color-medium);
  color: var(--ion-color-medium-contrast);
}
</style>
