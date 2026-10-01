<script setup lang="ts">
import { computed } from "vue";
import { IonPage } from "@ionic/vue";
import { useRoute } from "vue-router";
import ChatThread from "../components/ChatThread.vue";

const route = useRoute();
const chatId = computed(() => String(route.params.id));
// Plan 10.4: the games tab opens a game here (`?play=<id>`).
const play = computed(() => (typeof route.query?.play === "string" && route.query.play ? route.query.play : undefined));
</script>

<template>
  <ion-page>
    <ChatThread :chat-id="chatId" :show-back="true" :play="play" />
  </ion-page>
</template>
