<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IonButton, IonIcon, IonItem, IonLabel, IonList, IonModal } from "@ionic/vue";
import { extensionPuzzleOutline } from "ionicons/icons";
import { t } from "../i18n";

// Ioan, 2026-10-08: «si no tiene permiso se debería pedir siempre». A tool that asks for something
// it was not granted (installing grants nothing, §53) asks for that one permission on the spot, in
// Ionic's sheet modal as the game's permissions sheet: the tool's icon or image, its name, the
// permission with its label and icon, and Cancel / Allow. Dismissing it (dragging it down, a tap
// outside, the back button) is a no.
const props = defineProps<{
  open: boolean;
  name: string;
  permission?: { label: string; icon: string };
  icon?: string;
  image?: string;
}>();
const emit = defineEmits<{ allow: []; cancel: [] }>();

// What was asked stays on the sheet while it slides away.
const shown = ref({ name: props.name, permission: props.permission, icon: props.icon, image: props.image });
/** Whether the user already answered; Ionic's dismissal that follows says nothing more. */
let answered = false;
watch(
  () => [props.open, props.name, props.permission, props.icon, props.image] as const,
  ([open]) => {
    if (!open) return;
    shown.value = { name: props.name, permission: props.permission, icon: props.icon, image: props.image };
    answered = false;
  },
);
const title = computed(() => t("plugins.askPermission", { name: shown.value.name }));

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
  <!-- Ionic takes the modal's name once, when it is made: a new one for each question. -->
  <ion-modal
    :key="title + (shown.permission?.label ?? '')"
    :is-open="open"
    class="ft-permission-ask"
    :breakpoints="[0, 1]"
    :initial-breakpoint="1"
    :aria-label="title"
    @did-dismiss="dismissed"
  >
    <div class="ion-padding ion-text-center ft-permission-ask__body" data-test="permission-ask">
      <img v-if="shown.image" :src="shown.image" alt="" draggable="false" class="ft-permission-ask__image" />
      <ion-icon v-else :icon="shown.icon || extensionPuzzleOutline" color="primary" class="ft-permission-ask__icon" aria-hidden="true" />
      <h2>{{ title }}</h2>
      <ion-list v-if="shown.permission" lines="none" class="ft-permission-ask__list">
        <ion-item data-test="permission-asked">
          <ion-icon slot="start" :icon="shown.permission.icon" color="primary" aria-hidden="true" />
          <ion-label class="ion-text-wrap">{{ shown.permission.label }}</ion-label>
        </ion-item>
      </ion-list>
      <div class="ft-permission-ask__actions">
        <ion-button fill="outline" shape="round" data-test="permission-cancel" @click="answer(false)">
          {{ $t("common.cancel") }}
        </ion-button>
        <ion-button shape="round" data-test="permission-allow" @click="answer(true)">
          {{ $t("plugins.allow") }}
        </ion-button>
      </div>
    </div>
  </ion-modal>
</template>

<style scoped>
/* As tall as what it says (Ionic's breakpoints are shares of the sheet's own height). */
.ft-permission-ask {
  --height: auto;
}
/* The same room under the grab handle as the app's sheet, and above Android's navigation bar
   when the app runs edge to edge; clear of the side insets (a phone held sideways), which are
   physical. As the game's permissions sheet. */
.ft-permission-ask__body {
  padding-top: 26px;
  padding-bottom: calc(var(--ion-padding, 16px) + var(--ion-safe-area-bottom, 0px));
  padding-left: calc(var(--ion-padding, 16px) + var(--ion-safe-area-left, 0px));
  padding-right: calc(var(--ion-padding, 16px) + var(--ion-safe-area-right, 0px));
}
.ft-permission-ask__icon {
  font-size: 44px;
}
/* The tool's own image, as its tile draws it. */
.ft-permission-ask__image {
  display: block;
  width: 64px;
  height: 64px;
  margin: 0 auto;
  border-radius: 18px;
  object-fit: cover;
}
/* The one permission, as its line in the app's sheet: icon and label, read from the start. */
.ft-permission-ask__list {
  margin-block-end: var(--ft-space-3);
  text-align: start;
  background: transparent;
}
.ft-permission-ask__list ion-item {
  --background: transparent;
}
.ft-permission-ask__actions {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: var(--ft-space-2);
}
/* The app's buttons say things as written, not in Material's capitals. */
.ft-permission-ask__actions ion-button {
  text-transform: none;
}
</style>
