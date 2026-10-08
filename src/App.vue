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

// A reminder notification opened the app (2026-09-27): straight to the plugin that set it, in the
// hidden session it was set in if it was (2026-10-01, §108). Asked when the app starts and each
// time it comes back, since a tap may only bring it to the front.
const router = useRouter();
async function openTappedReminder() {
  const tapped = await pendingReminder();
  if (!tapped) return;
  const session = tapped.session ? `&session=${encodeURIComponent(tapped.session)}` : "";
  await router.push(`/plugin/${tapped.plugin}?reminder=${encodeURIComponent(tapped.id)}${session}`);
}
// Out of the foreground the core lets go of the router (2026-10-01; the platform tells it, so that
// the router pushes what comes): back on the screen, or opened from a push, it connects again at
// once, and the router's welcome fetches what waits. The push token goes over again too: a
// registration that failed (the router being deployed) must not leave the phone unreachable.
function onVisible() {
  if (document.visibilityState !== "visible") return;
  void resumeRouter();
  if (isOnboarded()) void enablePush();
  void openTappedReminder();
  // A link may only have brought the app to the front (2026-10-08).
  void checkOpenedLink();
  // The plan may have changed while the app was away (the free days ended): the tools' lock too.
  void refreshPremiumLock();
}
// An update the core made in the background reaches every list of plugins (2026-10-03).
let unfollow: (() => void) | undefined;
// Whether the tools are locked (Ioan, 2026-10-08), followed while the app runs.
let unfollowLock: (() => void) | undefined;
// Links that open the app (2026-10-08, `opened.ts`): asked for at start, back on the screen and
// when the core says one came while the app runs.
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
