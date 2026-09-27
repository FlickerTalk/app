<script setup lang="ts">
import { onMounted } from "vue";
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import { useRouter } from "vue-router";
import IncomingCall from "./components/IncomingCall.vue";
import ScannerOverlay from "./components/ScannerOverlay.vue";
import { pendingReminder } from "./core";

// A reminder notification opened the app (2026-09-27): straight to the plugin that set it.
const router = useRouter();
onMounted(async () => {
  const tapped = await pendingReminder();
  if (tapped) await router.push(`/plugin/${tapped.plugin}?reminder=${encodeURIComponent(tapped.id)}`);
});
</script>

<template>
  <ion-app>
    <ion-router-outlet />
    <IncomingCall />
    <ScannerOverlay />
  </ion-app>
</template>
