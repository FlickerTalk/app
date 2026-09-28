<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import { useRouter } from "vue-router";
import IncomingCall from "./components/IncomingCall.vue";
import ScannerOverlay from "./components/ScannerOverlay.vue";
import { pendingReminder, resumeRouter } from "./core";

// A reminder notification opened the app (2026-09-27): straight to the plugin that set it. Asked
// when the app starts and each time it comes back, since a tap may only bring it to the front.
const router = useRouter();
async function openTappedReminder() {
  const tapped = await pendingReminder();
  if (tapped) await router.push(`/plugin/${tapped.plugin}?reminder=${encodeURIComponent(tapped.id)}`);
}
// iOS cuts the socket of a suspended app (2026-09-28): back on the screen, or opened from a push,
// reconnect at once, and the router's welcome fetches what waits.
function onVisible() {
  if (document.visibilityState !== "visible") return;
  void resumeRouter();
  void openTappedReminder();
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
