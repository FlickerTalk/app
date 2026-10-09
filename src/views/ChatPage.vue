<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IonPage, onIonViewDidEnter, onIonViewWillEnter, onIonViewWillLeave } from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import ChatThread from "../components/ChatThread.vue";
import { takeSearch } from "../pending-search";

const route = useRoute();
// The conversation of this page, for as long as it lives: Ionic gives each one a page of its own
// and keeps it mounted under the next page and while it goes, when the address is another's.
const chatId = String(route.params.id);
// Plan 10.4: the games tab opens a game here (`?play=<id>`).
const play = computed(() => (typeof route.query?.play === "string" && route.query.play ? route.query.play : undefined));
// Ioan, 2026-10-06: the contact page's search opens the conversation with `?search=1`, only for
// this page's conversation (another page's address may be on screen).
const search = computed(() => String(route.params.id) === chatId && route.query?.search === "1");
// Under another page, what the conversation left open lets go of Android's back button.
const onScreen = ref(true);
onIonViewDidEnter(() => {
  onScreen.value = true;
  // Back from the contact page's search (2026-10-06): the conversation searches now it is seen.
  if (takeSearch(chatId)) thread.value?.openSearch();
});

// Left by going back (2026-10-02), the page is taken down once Ionic's transition ends, and with it
// the plugin or game open in it: it is closed as the page starts to go, so its goodbye goes out
// meanwhile. Going back is told by vue-router's history position, lower than when the page came
// on screen. A page covered by another one (a push) keeps what it has open, as before; a back
// button with no history replaces the page at the same position, and only its teardown is left.
const router = useRouter();
const thread = ref<InstanceType<typeof ChatThread> | null>(null);
let enteredAt: number | undefined;
const position = () => {
  const at = router.options.history.state?.position;
  return typeof at === "number" ? at : undefined;
};
// Once open, the address forgets it, so coming back to the conversation does not open it again.
watch(
  search,
  (asked) => {
    if (!asked) return;
    const { search: _asked, ...rest } = route.query;
    void router.replace({ query: rest });
  },
  { immediate: true },
);
onIonViewWillEnter(() => (enteredAt = position()));
onIonViewWillLeave(() => {
  onScreen.value = false;
  const now = position();
  if (enteredAt !== undefined && now !== undefined && now < enteredAt) thread.value?.leave();
});
</script>

<template>
  <ion-page class="ft-thread">
    <!-- The thread's header, content and footer are this page's own children (Ionic's shape). A
         comment before the page would make the component's root a fragment. -->
    <ChatThread ref="thread" :chat-id="chatId" :show-back="true" :play="play" :search="search" :active="onScreen" />
  </ion-page>
</template>

<style scoped>
/* Ionic's page already lays out the thread's header, content and footer in a column. */
.ft-thread {
  background: var(--ft-bg);
}
</style>
