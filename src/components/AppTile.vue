<script setup lang="ts">
import { computed, onBeforeUnmount } from "vue";
import { IonIcon, IonRippleEffect, IonSpinner } from "@ionic/vue";
import { addOutline, downloadOutline, lockClosedOutline } from "ionicons/icons";
import { t } from "../i18n";

// 2026-10-08 (plan of the apps grid, docs/mockups/apps-grid.html): an app as a home-screen icon,
// its name under it. A tap opens it (or installs it); touching and holding shows what it is.
const props = defineProps<{
  name: string;
  /** The icon as an SVG data URL (`pluginIcon`). */
  icon: string;
  /** A download, an app the phone carries (added, not downloaded), or the premium lock. */
  badge?: "download" | "add" | "lock";
  /** Not installed: dimmed. */
  off?: boolean;
  /** Under the name: what a download weighs. */
  caption?: string;
  /** Installing: a spinner over the icon, in the icon's own box. */
  busy?: boolean;
  /** How a screen reader names it, when the name alone is not enough. */
  label?: string;
}>();
const emit = defineEmits<{ tap: []; hold: [] }>();

/** How long a press must last to be a hold, and how far the finger may wander meanwhile. */
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
/** A hold just happened: the click that ends the press is not a tap. */
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
  if (event.button !== 0) return;
  start = { x: event.clientX, y: event.clientY };
  timer = setTimeout(hold, HOLD_MS);
}

function move(event: PointerEvent) {
  if (timer === undefined) return;
  if (Math.hypot(event.clientX - start.x, event.clientY - start.y) > SLOP_PX) cancel();
}

function click() {
  if (held) {
    held = false;
    return;
  }
  emit("tap");
}

/**
 * A right click, or the menu Android opens on a long press: one hold, never the browser's menu.
 * During a press (the timer running) it is that press's hold, and the click ending it is no tap;
 * a right click has no click after it, so nothing is held over for the next one.
 */
function menu(event: Event) {
  event.preventDefault();
  if (held) return;
  if (timer !== undefined) return hold();
  emit("hold");
}

/** Enter and Space click the button by themselves; Shift+Enter holds. */
function key(event: KeyboardEvent) {
  if (event.key !== "Enter" || !event.shiftKey) return;
  event.preventDefault();
  emit("hold");
}

onBeforeUnmount(cancel);
</script>

<template>
  <button
    type="button"
    class="ft-app-tile ion-activatable"
    :class="{ 'ft-app-tile--off': off }"
    :aria-label="ariaLabel"
    :aria-busy="busy ? 'true' : undefined"
    :disabled="busy"
    @pointerdown="down"
    @pointermove="move"
    @pointerup="cancel"
    @pointercancel="cancel"
    @pointerleave="cancel"
    @click="click"
    @contextmenu="menu"
    @keydown="key"
  >
    <span class="ft-app-tile__icon">
      <ion-icon :icon="icon" aria-hidden="true" />
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
    <ion-ripple-effect />
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
  /* A long press is the app's, not the WebView's: no text selection, no iOS callout. */
  -webkit-user-select: none;
  user-select: none;
  -webkit-touch-callout: none;
  -webkit-tap-highlight-color: transparent;
}
.ft-app-tile:disabled {
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
/* Over the icon, inside its box: the tile never changes size while it installs. */
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
.ft-app-tile__name {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  line-clamp: 2;
  overflow: hidden;
  box-sizing: border-box;
  max-width: 100%;
  padding-inline: 2px;
  font-size: 13px;
  line-height: 1.25;
  text-align: center;
  overflow-wrap: anywhere;
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
