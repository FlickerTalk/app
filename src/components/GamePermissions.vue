<script setup lang="ts">
import { ref, watch } from "vue";
import { IonButton, IonIcon, IonModal } from "@ionic/vue";
import { gameControllerOutline } from "ionicons/icons";
import { formatSize } from "../core";

// Plan decision 11: installing grants nothing (§53). The first time a game is played, this one
// sheet says what it will do —talk to the other person's phone, leave the result in the chat— and
// one button grants both. A game that is not here yet is a download: the sheet says what it
// weighs, and the same button installs it. Without a yes, nothing is installed or granted.
// Ioan, 2026-10-02: Ionic's sheet modal, as tall as what it says; dismissing it (dragging it down,
// a tap outside, the back button) is a no.
// The game's own icon (its tile's, `pluginIcon`), or the controller (device review of app#121).
// Or its own image (`pluginImage`, 2026-10-08), drawn instead of any icon.
// 2026-10-08: presenting in a call asks with the same sheet, with its own text and button.
const props = defineProps<{ open: boolean; name: string; size?: number; icon?: string; image?: string; body?: string; allowLabel?: string }>();
const emit = defineEmits<{ allow: []; cancel: [] }>();

// What was asked stays on the sheet while it slides away.
const shown = ref({ name: props.name, size: props.size, icon: props.icon, image: props.image, body: props.body, allowLabel: props.allowLabel });
/** Whether the user already answered; Ionic's dismissal that follows says nothing more. */
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
  <!-- Ionic takes the modal's name once, when it is made: a new one for each game asked about
       (the name only changes while the sheet is closed). -->
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
/* As tall as what it says (Ionic's breakpoints are shares of the sheet's own height). */
.ft-game-ask {
  --height: auto;
}
/* Above Android's navigation bar when the app runs edge to edge, as the composer keeps itself. */
.ft-game-ask__body {
  /* The same room under the grab handle as the app's sheet (second device review). */
  padding-top: 26px;
  padding-bottom: calc(var(--ion-padding, 16px) + var(--ion-safe-area-bottom, 0px));
  /* And clear of the side insets (a phone held sideways), which are physical. */
  padding-left: calc(var(--ion-padding, 16px) + var(--ion-safe-area-left, 0px));
  padding-right: calc(var(--ion-padding, 16px) + var(--ion-safe-area-right, 0px));
}
.ft-game-ask__icon {
  font-size: 44px;
}
/* The game's own image, as its tile draws it (2026-10-08). */
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
/* The app's buttons say things as written, not in Material's capitals. */
.ft-game-ask__actions ion-button {
  text-transform: none;
}
</style>
