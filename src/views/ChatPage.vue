<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IonPage, onIonViewDidEnter, onIonViewWillEnter, onIonViewWillLeave } from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import ChatThread from "../components/ChatThread.vue";
import { useOnScreen } from "../on-screen";
import { takeSearch } from "../pending-search";

const route = useRoute();
// read once, Ionic keeps this page mounted while the address already names another chat
const chatId = String(route.params.id);
const forThisChat = () => String(route.params.id) === chatId;
const play = computed(() => {
  const asked = route.query?.play;
  return forThisChat() && typeof asked === "string" && asked ? asked : undefined;
});
const search = computed(() => forThisChat() && route.query?.search === "1");
const onScreen = useOnScreen();
onIonViewDidEnter(() => {
  if (takeSearch(chatId)) thread.value?.openSearch();
});

// going back closes the plugin as the page starts to leave, so its goodbye goes out during the transition
const router = useRouter();
const thread = ref<InstanceType<typeof ChatThread> | null>(null);
let enteredAt: number | undefined;
const position = () => {
  const at = router.options.history.state?.position;
  return typeof at === "number" ? at : undefined;
};
watch(
  [search, play],
  ([searching, playing]) => {
    if (!searching && !playing) return;
    const { search: _search, play: _play, ...rest } = route.query;
    void router.replace({ query: rest });
  },
  { immediate: true },
);
onIonViewWillEnter(() => (enteredAt = position()));
onIonViewWillLeave(() => {
  const now = position();
  if (enteredAt !== undefined && now !== undefined && now < enteredAt) thread.value?.leave();
});
</script>

<template>
  <ion-page class="ft-thread">
    <!-- a comment before ion-page would make the root a fragment, keep them inside -->
    <ChatThread ref="thread" :chat-id="chatId" :show-back="true" :play="play" :search="search" :active="onScreen" />
  </ion-page>
</template>

<style scoped>
.ft-thread {
  background: var(--ft-bg);
}
</style>
