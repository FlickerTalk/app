<script setup lang="ts">
import { computed } from "vue";
import { IonContent, IonHeader, IonIcon, IonItem, IonLabel, IonList, IonListHeader, IonModal, IonTitle, IonToolbar } from "@ionic/vue";
import { lockClosedOutline, personAddOutline } from "ionicons/icons";
import Avatar from "./Avatar.vue";
import { store } from "../core";
import { t } from "../i18n";

// Plan 10.4: who a game is played with (the Apps tab). Ioan, 2026-10-09: also who a tool opened on
// its own sends what it proposes to. A sheet over the page, half the height first; whoever shows it
// says where it sits on a wide screen (`ft-sheet--tab`, `ft-sheet--window`) by its class.
const props = defineProps<{ open: boolean; purpose: "play" | "send" }>();
const emit = defineEmits<{ pick: [contact: string]; add: []; dismiss: [] }>();

const title = computed(() => t(props.purpose === "play" ? "games.pickContact" : "apps.sendTo"));
const nobody = computed(() => t(props.purpose === "play" ? "games.noContacts" : "apps.noContactsToSend"));
const rowTest = (id: string) => `${props.purpose === "play" ? "play-with" : "send-to"}-${id}`;

/**
 * The contacts of the main list and of each open hidden session (§108), never a blocked one.
 * Strangers who wrote first are not contacts yet.
 */
const places = computed(() =>
  [
    { id: "", chats: store.chats },
    ...store.sessions.filter((session) => session.id).map((session) => ({ id: session.id, chats: session.chats })),
  ]
    .map((place) => ({ id: place.id, chats: place.chats.filter((chat) => !chat.blocked) }))
    .filter((place) => place.chats.length),
);
</script>

<template>
  <ion-modal
    :is-open="open"
    class="ft-contact-picker"
    :breakpoints="[0, 0.5, 1]"
    :initial-breakpoint="0.5"
    :expand-to-scroll="false"
    :aria-label="title"
    @did-dismiss="emit('dismiss')"
  >
    <ion-header>
      <ion-toolbar>
        <ion-title><span class="ft-title">{{ title }}</span></ion-title>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ft-contact-picker__content" data-test="contact-picker">
      <!-- Nobody yet: the way to add someone (device review of app#121). -->
      <ion-list v-if="!places.length">
        <ion-item button :detail="false" lines="none" data-test="picker-add-contact" @click="emit('add')">
          <ion-icon slot="start" :icon="personAddOutline" color="primary" aria-hidden="true" />
          <ion-label color="primary">{{ nobody }}</ion-label>
        </ion-item>
      </ion-list>
      <ion-list v-for="place in places" :key="place.id">
        <!-- A hidden session's contacts, under their own heading. -->
        <ion-list-header v-if="place.id">
          <ion-icon :icon="lockClosedOutline" aria-hidden="true" />
        </ion-list-header>
        <ion-item v-for="chat in place.chats" :key="chat.id" button :detail="false" :data-test="rowTest(chat.id)" @click="emit('pick', chat.id)">
          <Avatar slot="start" :name="chat.name" :hue="chat.hue" :size="36" />
          <ion-label class="ion-text-nowrap" dir="auto">{{ chat.name }}</ion-label>
        </ion-item>
      </ion-list>
    </ion-content>
  </ion-modal>
</template>

<style scoped>
/* The list scrolls to its last contact above Android's navigation bar (edge to edge). */
.ft-contact-picker__content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
</style>
