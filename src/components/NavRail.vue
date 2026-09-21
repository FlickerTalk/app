<script setup lang="ts">
import { IonIcon } from "@ionic/vue";
import {
  call,
  callOutline,
  chatbubbles,
  chatbubblesOutline,
  settings,
  settingsOutline,
} from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import data from "../mock/chats.json";

const sections = [
  { path: "/tabs/chats", label: "Chats", icon: chatbubblesOutline, activeIcon: chatbubbles },
  { path: "/tabs/calls", label: "Calls", icon: callOutline, activeIcon: call },
  { path: "/tabs/settings", label: "Settings", icon: settingsOutline, activeIcon: settings },
];

const route = useRoute();
const router = useRouter();

const isActive = (path: string) => route.path.startsWith(path);
</script>

<template>
  <nav class="ft-rail" aria-label="Main">
    <span class="ft-rail__mark" aria-hidden="true">
      <svg viewBox="0 0 32 32" width="30" height="30" fill="none">
        <circle cx="8" cy="24" r="4" fill="currentColor" opacity="0.55" />
        <circle cx="24" cy="8" r="4" fill="currentColor" />
        <path
          d="M11.5 20.5 20.5 11.5"
          stroke="currentColor"
          stroke-width="2.5"
          stroke-linecap="round"
          stroke-dasharray="3 4"
        />
      </svg>
    </span>

    <button
      v-for="section in sections"
      :key="section.path"
      type="button"
      class="ft-rail__item"
      :class="{ 'is-active': isActive(section.path) }"
      :aria-label="section.label"
      :aria-current="isActive(section.path) ? 'page' : undefined"
      :title="section.label"
      @click="router.push(section.path)"
    >
      <span class="ft-rail__pill">
        <ion-icon :icon="isActive(section.path) ? section.activeIcon : section.icon" aria-hidden="true" />
      </span>
    </button>

    <span class="ft-rail__spacer" />
    <Avatar :name="data.me.name" :hue="data.me.hue" :size="36" />
  </nav>
</template>

<style scoped>
.ft-rail {
  position: absolute;
  inset: 0 auto 0 0;
  z-index: 10;
  display: none;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-3);
  width: var(--ft-rail-width);
  padding: calc(env(safe-area-inset-top) + var(--ft-space-5)) 0 var(--ft-space-5);
  background: var(--ft-surface);
  border-right: 1px solid var(--ft-border);
}
@media (min-width: 768px) {
  .ft-rail {
    display: flex;
  }
}

.ft-rail__mark {
  display: grid;
  place-items: center;
  margin-bottom: var(--ft-space-4);
  color: var(--ft-accent);
  filter: drop-shadow(0 0 6px var(--ft-glow));
}

.ft-rail__item {
  appearance: none;
  display: grid;
  place-items: center;
  width: 56px;
  height: 40px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--ft-muted);
  cursor: pointer;
}
.ft-rail__pill {
  display: grid;
  place-items: center;
  width: 56px;
  height: 32px;
  border-radius: 16px;
  font-size: 22px;
  transition:
    background 0.2s,
    color 0.2s;
}
.ft-rail__item:hover .ft-rail__pill {
  background: var(--ft-surface-2);
}
.ft-rail__item.is-active {
  color: var(--ft-accent);
}
.ft-rail__item.is-active .ft-rail__pill {
  background: color-mix(in srgb, var(--ft-accent) 16%, transparent);
}
.ft-rail__item:focus-visible .ft-rail__pill {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}

.ft-rail__spacer {
  flex: 1;
}
</style>
