<script setup lang="ts">
import { computed, ref } from "vue";
import { IonPage, onIonViewDidEnter, onIonViewWillLeave } from "@ionic/vue";
import { useRoute } from "vue-router";
import ChatThread from "../components/ChatThread.vue";

const route = useRoute();
const chatId = computed(() => String(route.params.id));
// Plan 10.4: the games tab opens a game here (`?play=<id>`).
const play = computed(() => (typeof route.query?.play === "string" && route.query.play ? route.query.play : undefined));
// Under another page, what the conversation left open lets go of Android's back button.
const onScreen = ref(true);
onIonViewWillLeave(() => (onScreen.value = false));
onIonViewDidEnter(() => (onScreen.value = true));
</script>

<template>
  <ion-page>
    <ChatThread :chat-id="chatId" :show-back="true" :play="play" :active="onScreen" />
  </ion-page>
</template>
