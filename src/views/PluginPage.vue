<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import PluginSheet from "../components/PluginSheet.vue";
import { installed, refreshPlugins } from "../plugins";

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
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ plugin?.name ?? "" }}</ion-title>
      </ion-toolbar>
    </ion-header>
    <ion-content>
      <PluginSheet
        v-if="ready && plugin"
        :plugin="plugin"
        contact=""
        :reminder="reminder"
        :session="session"
        @open-chat="(contact) => router.push(`/chat/${contact}`)"
        @done="router.back()"
      />
      <p v-else-if="ready" class="ft-plugin-page__missing">{{ $t("plugins.none") }}</p>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-plugin-page__missing {
  padding: var(--ft-space-4);
  color: var(--ft-muted);
  text-align: center;
}
</style>
