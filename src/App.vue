<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import { useRouter } from "vue-router";
import IncomingCall from "./components/IncomingCall.vue";
import ScannerOverlay from "./components/ScannerOverlay.vue";
import { pendingReminder } from "./core";

// A reminder notification opened the app (2026-09-27): straight to the plugin that set it. Asked
// when the app starts and each time it comes back, since a tap may only bring it to the front.
const router = useRouter();
async function openTappedReminder() {
  const tapped = await pendingReminder();
  if (tapped) await router.push(`/plugin/${tapped.plugin}?reminder=${encodeURIComponent(tapped.id)}`);
}
function onVisible() {
  if (document.visibilityState === "visible") void openTappedReminder();
}
onMounted(() => {
  document.addEventListener("visibilitychange", onVisible);
  void openTappedReminder();
});
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));
</script>

<template>
  <ion-app>
    <ion-router-outlet />
    <IncomingCall />
    <ScannerOverlay />
  </ion-app>
</template>
