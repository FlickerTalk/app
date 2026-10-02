<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonPage,
  IonTitle,
  IonToolbar,
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import PluginSheet from "../components/PluginSheet.vue";
import { installed, pluginName, refreshPlugins } from "../plugins";

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
  if (!plugin.value) await refreshPlugins();
  ready.value = true;
});

// 2026-10-02: leaving the page closes the plugin through its sheet, so it can say goodbye while the
// page goes (Ionic keeps it a moment, and under the next page if one is pushed). Back on the page
// after that, the plugin is opened afresh (`opening`, a frame of its own).
const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
const opening = ref(0);
let here = false;
let saidGoodbye = false;
onIonViewWillEnter(() => {
  here = true;
  if (saidGoodbye) reopen();
});
onIonViewWillLeave(() => {
  here = false;
  void sheet.value?.close();
});
function closed() {
  if (here) reopen();
  else saidGoodbye = true;
}
function reopen() {
  saidGoodbye = false;
  opening.value += 1;
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ plugin ? pluginName(plugin) : "" }}</ion-title>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ft-plugin-page">
      <PluginSheet
        v-if="ready && plugin"
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
.ft-plugin-page__missing {
  padding: var(--ft-space-4);
  color: var(--ft-muted);
  text-align: center;
}
</style>
