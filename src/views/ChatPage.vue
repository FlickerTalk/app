<script setup lang="ts">
import { computed } from "vue";
import { IonPage } from "@ionic/vue";
import { useRoute } from "vue-router";
import ChatThread from "../components/ChatThread.vue";

const route = useRoute();
const chatId = computed(() => String(route.params.id));
</script>

<template>
  <ion-page>
    <ChatThread :chat-id="chatId" :show-back="true" />
  </ion-page>
</template>
