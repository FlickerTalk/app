<script setup lang="ts">
import { onMounted, onUnmounted } from "vue";
import { IonApp, IonRouterOutlet } from "@ionic/vue";
import { useRouter } from "vue-router";
import CallBar from "./components/CallBar.vue";
import IncomingCall from "./components/IncomingCall.vue";
import ScannerOverlay from "./components/ScannerOverlay.vue";
import { enablePush, pendingReminder, resumeRouter } from "./core";
import { checkOpenedLink, startOpenedLinks } from "./opened";
import { followPluginChanges, followPremiumLock, refreshPremiumLock } from "./plugins";
import { isOnboarded } from "./preferences";

// a tap on the notification may only bring the app to the front
const router = useRouter();
async function openTappedReminder() {
  const tapped = await pendingReminder();
  if (!tapped) return;
  const session = tapped.session ? `&session=${encodeURIComponent(tapped.session)}` : "";
  await router.push(`/plugin/${tapped.plugin}?reminder=${encodeURIComponent(tapped.id)}${session}`);
}
// out of the foreground the core lets go of the router. Register the push token again too,
// a registration that failed during a router deploy must not leave the phone unreachable.
function onVisible() {
  if (document.visibilityState !== "visible") return;
  void resumeRouter();
  if (isOnboarded()) void enablePush();
  void openTappedReminder();
  // a link may only have brought the app to the front
  void checkOpenedLink();
  void refreshPremiumLock();
}
let unfollow: (() => void) | undefined;
let unfollowLock: (() => void) | undefined;
let stopLinks: (() => void) | undefined;
onMounted(() => {
  document.addEventListener("visibilitychange", onVisible);
  void openTappedReminder();
  stopLinks = startOpenedLinks(router);
  void checkOpenedLink();
  void followPluginChanges()
    .then((stop) => (unfollow = stop))
    .catch(() => undefined);
  void followPremiumLock()
    .then((stop) => (unfollowLock = stop))
    .catch(() => undefined);
});
onUnmounted(() => {
  document.removeEventListener("visibilitychange", onVisible);
  unfollow?.();
  unfollowLock?.();
  stopLinks?.();
});
</script>

<template>
  <ion-app>
    <ion-router-outlet />
    <IncomingCall />
    <CallBar />
    <ScannerOverlay />
  </ion-app>
</template>
