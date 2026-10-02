<script setup lang="ts">
import { formatSize } from "../core";

// Plan decision 11: installing grants nothing (§53). The first time a game is played, this one
// sheet says what it will do —talk to the other person's phone, leave the result in the chat— and
// one button grants both. A game that is not here yet is a download: the sheet says what it
// weighs, and the same button installs it. Without a yes, nothing is installed or granted.
const props = defineProps<{ name: string; size?: number }>();
const emit = defineEmits<{ allow: []; cancel: [] }>();
</script>

<template>
  <div class="ft-game-ask" data-test="game-permissions" role="dialog" :aria-label="props.name" @click.self="emit('cancel')">
    <div class="ft-game-ask__card">
      <span class="ft-game-ask__icon" aria-hidden="true">🎮</span>
      <h2 class="ft-game-ask__name">{{ props.name }}</h2>
      <p class="ft-game-ask__body">{{ $t("games.permissionsBody") }}</p>
      <p v-if="props.size !== undefined" class="ft-game-ask__size">{{ formatSize(props.size) }}</p>
      <div class="ft-game-ask__actions">
        <button type="button" class="ft-game-ask__cancel" data-test="game-cancel" @click="emit('cancel')">
          {{ $t("common.cancel") }}
        </button>
        <button type="button" class="ft-game-ask__allow" data-test="game-allow" @click="emit('allow')">
          {{ props.size !== undefined ? $t("games.installAndPlay") : $t("games.permissionsAllow") }}
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.ft-game-ask {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: grid;
  place-items: end center;
  padding: var(--ft-space-4);
  /* Above Android's navigation bar when the app runs edge to edge, as the composer keeps itself. */
  padding-bottom: calc(var(--ft-space-4) + var(--ion-safe-area-bottom, 0px));
  background: rgba(0, 0, 0, 0.35);
}
.ft-game-ask__card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-2);
  width: min(100%, 420px);
  padding: var(--ft-space-5) var(--ft-space-4) var(--ft-space-4);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
  color: var(--ft-text);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.3);
  text-align: center;
}
.ft-game-ask__icon {
  font-size: 40px;
  line-height: 1;
}
.ft-game-ask__name {
  margin: 0;
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-game-ask__body {
  margin: 0;
  color: var(--ft-muted);
  font-size: 15px;
  line-height: 1.4;
}
.ft-game-ask__size {
  margin: 0;
  color: var(--ft-muted);
  font-size: 13px;
}
.ft-game-ask__actions {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: var(--ft-space-3);
  margin-top: var(--ft-space-3);
}
.ft-game-ask__allow,
.ft-game-ask__cancel {
  min-width: 120px;
  min-height: 44px;
  padding: 0 18px;
  border-radius: 999px;
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-game-ask__allow {
  border: 0;
  color: var(--ft-on-accent);
  background: var(--ft-accent);
}
.ft-game-ask__cancel {
  border: 1px solid var(--ft-border);
  color: var(--ft-text);
  background: transparent;
}
</style>
