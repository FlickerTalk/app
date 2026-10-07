<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonPage,
  IonTitle,
  IonToolbar,
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import { closeOnBackWhile } from "../back";
import PluginSheet from "../components/PluginSheet.vue";
import { lockClosedOutline } from "ionicons/icons";
import { installed, isLocked, pluginName, refreshPlugins, refreshToolsLock } from "../plugins";

// A plugin on its own (2026-09-27): from Settings, or from a reminder it set. There is no chat
// behind it, so it cannot write in one nor talk to another side; it can open the conversation a
// ref of its came from.
// Opened from a reminder set inside a hidden session, it is open in that session (2026-10-01,
// §108); from Settings, in none.
const route = useRoute();
const router = useRouter();
const id = String(route.params.id);
// Read from the address each time: a reminder tapped while this page is on screen only changes it.
const reminder = computed(() => (typeof route.query.reminder === "string" ? route.query.reminder : undefined));
const session = computed(() => (typeof route.query.session === "string" && route.query.session ? route.query.session : undefined));
const plugin = computed(() => installed.value.find((one) => one.id === id));
const ready = ref(false);

onMounted(async () => {
  await Promise.all([plugin.value ? undefined : refreshPlugins(), refreshToolsLock()]);
  ready.value = true;
});
// Ioan, 2026-10-08: a tool opened on its own while the tools are locked (the free year over, no
// subscription) shows the lock and the way to the subscription, never its frame. Games never are.
const locked = computed(() => Boolean(plugin.value && isLocked(plugin.value)));

// 2026-10-02: leaving the page closes the plugin through its sheet, so it can say goodbye while the
// page goes (Ionic keeps it a moment, and under the next page if one is pushed). Back on the page
// after that, the plugin is opened afresh (`opening`, a frame of its own).
const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
const opening = ref(0);
const here = ref(false);
let saidGoodbye = false;
onIonViewWillEnter(() => {
  here.value = true;
  if (saidGoodbye) reopen();
});
onIonViewWillLeave(() => {
  here.value = false;
  void sheet.value?.close();
});
function closed() {
  if (here.value) reopen();
  else saidGoodbye = true;
}
function reopen() {
  saidGoodbye = false;
  opening.value += 1;
}

// Android's back button (issue #61, 2026-10-03): left to the system, Back follows the WebView's
// history, and Android's WebView skips the entries added without a touch on the page (a reminder
// tapped in the notifications opens this page that way). With nothing left to go back to, Android
// put the app away, still on the plugin. While the page is on screen the app takes the button and
// goes where the back arrow goes: back, or to Settings with nothing behind.
function leave() {
  if (window.history.state?.back) router.back();
  else void router.replace("/tabs/settings");
}
closeOnBackWhile(() => here.value, leave);
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ plugin ? pluginName(plugin) : "" }}</span></ion-title>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ft-plugin-page">
      <div v-if="ready && plugin && locked" class="ft-plugin-page__missing" data-test="locked">
        <ion-icon :icon="lockClosedOutline" class="ft-plugin-page__lock" aria-hidden="true" />
        <p>{{ $t("plugins.lockedHint") }}</p>
        <ion-button data-test="subscribe" @click="router.push('/plan')">{{ $t("plugins.subscribe") }}</ion-button>
      </div>
      <PluginSheet
        v-else-if="ready && plugin"
        ref="sheet"
        :key="opening"
        :plugin="plugin"
        contact=""
        :reminder="reminder"
        :session="session"
        @open-chat="(contact) => router.push(`/chat/${contact}`)"
        @done="router.back()"
        @closed="closed"
      />
      <p v-else-if="ready" class="ft-plugin-page__missing">{{ $t("plugins.none") }}</p>
    </ion-content>
  </ion-page>
</template>

<style scoped>
/* The plugin's last pixel can be scrolled above Android's navigation bar (edge to edge). */
.ft-plugin-page {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-plugin-page__lock {
  font-size: 40px;
}
.ft-plugin-page__missing {
  padding: var(--ft-space-4);
  color: var(--ft-muted);
  text-align: center;
}
</style>
