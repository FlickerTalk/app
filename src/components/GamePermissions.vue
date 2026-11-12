<script setup lang="ts">
import { ref, watch } from "vue";
import { IonButton, IonIcon, IonModal } from "@ionic/vue";
import { gameControllerOutline } from "ionicons/icons";
import { formatSize } from "../core";

const props = defineProps<{ open: boolean; name: string; size?: number; icon?: string; image?: string; body?: string; allowLabel?: string }>();
const emit = defineEmits<{ allow: []; cancel: [] }>();

// keep showing what was asked while the sheet slides away
const shown = ref({ name: props.name, size: props.size, icon: props.icon, image: props.image, body: props.body, allowLabel: props.allowLabel });
// Ionic's own dismissal after an answer means nothing
let answered = false;
watch(
  () => [props.open, props.name, props.size, props.icon, props.image, props.body, props.allowLabel] as const,
  ([open]) => {
    if (!open) return;
    shown.value = { name: props.name, size: props.size, icon: props.icon, image: props.image, body: props.body, allowLabel: props.allowLabel };
    answered = false;
  },
);

function answer(yes: boolean) {
  answered = true;
  if (yes) emit("allow");
  else emit("cancel");
}

function dismissed() {
  if (!answered) emit("cancel");
  answered = false;
}
</script>

<template>
  <!-- Ionic reads the modal's name once, so each game gets a new modal -->
  <ion-modal
    :key="shown.name"
    :is-open="open"
    class="ft-game-ask"
    :breakpoints="[0, 1]"
    :initial-breakpoint="1"
    :aria-label="shown.name"
    @did-dismiss="dismissed"
  >
    <div class="ion-padding ion-text-center ft-game-ask__body" data-test="game-permissions">
      <img v-if="shown.image" :src="shown.image" alt="" draggable="false" class="ft-game-ask__image" />
      <ion-icon v-else :icon="shown.icon || gameControllerOutline" color="primary" class="ft-game-ask__icon" aria-hidden="true" />
      <h2>{{ shown.name }}</h2>
      <p>{{ shown.body ?? $t("games.permissionsBody") }}</p>
      <p v-if="shown.size !== undefined" class="ft-muted">{{ formatSize(shown.size) }}</p>
      <div class="ft-game-ask__actions">
        <ion-button fill="outline" shape="round" data-test="game-cancel" @click="answer(false)">
          {{ $t("common.cancel") }}
        </ion-button>
        <ion-button shape="round" data-test="game-allow" @click="answer(true)">
          {{ shown.size !== undefined ? $t("games.installAndPlay") : (shown.allowLabel ?? $t("games.permissionsAllow")) }}
        </ion-button>
      </div>
    </div>
  </ion-modal>
</template>

<style scoped>
.ft-game-ask {
  --height: auto;
}
.ft-game-ask__body {
  padding-top: 26px;
  padding-bottom: calc(var(--ion-padding, 16px) + var(--ion-safe-area-bottom, 0px));
  padding-left: calc(var(--ion-padding, 16px) + var(--ion-safe-area-left, 0px));
  padding-right: calc(var(--ion-padding, 16px) + var(--ion-safe-area-right, 0px));
}
.ft-game-ask__icon {
  font-size: 44px;
}
.ft-game-ask__image {
  display: block;
  width: 64px;
  height: 64px;
  margin: 0 auto;
  border-radius: 18px;
  object-fit: cover;
}
.ft-game-ask__actions {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: var(--ft-space-2);
}
.ft-game-ask__actions ion-button {
  text-transform: none;
}
</style>
