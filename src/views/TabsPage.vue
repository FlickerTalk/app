<script setup lang="ts">
import { IonIcon, IonPage, IonRouterOutlet, IonTabBar, IonTabButton, IonTabs } from "@ionic/vue";
import { appsOutline, callOutline, chatbubblesOutline, settingsOutline } from "ionicons/icons";
import NavRail from "../components/NavRail.vue";
import { t } from "../i18n";
import { useOnScreen } from "../on-screen";

const tabs = [
  { tab: "chats", href: "/tabs/chats", label: t("tabs.chats"), icon: chatbubblesOutline },
  { tab: "calls", href: "/tabs/calls", label: t("tabs.calls"), icon: callOutline },
  { tab: "apps", href: "/tabs/apps", label: t("tabs.apps"), icon: appsOutline },
  { tab: "settings", href: "/tabs/settings", label: t("tabs.settings"), icon: settingsOutline },
];

// Ionic tells a push over the tabs to this page only, never to the tab inside
useOnScreen();
</script>

<template>
  <ion-page>
    <NavRail />
    <ion-tabs class="ft-tabs-frame">
      <ion-router-outlet />
      <ion-tab-bar slot="bottom" class="ft-tab-bar">
        <ion-tab-button
          v-for="item in tabs"
          :key="item.tab"
          :tab="item.tab"
          :href="item.href"
          :aria-label="item.label"
        >
          <ion-icon :icon="item.icon" aria-hidden="true" />
        </ion-tab-button>
      </ion-tab-bar>
    </ion-tabs>
  </ion-page>
</template>

<style>
.ft-tab-bar {
  --border: 1px solid var(--ft-border);
  /* Ionic already pads the bar for the home indicator, adding it here doubled it on the iPhone */
  height: 56px;
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
}
.ft-tab-bar ion-tab-button {
  font-size: 13px;
}
.ft-tab-bar ion-tab-button ion-icon {
  font-size: 25px;
}

@media (min-width: 768px) {
  /* IonTabs sets inline left/right/width, override them past the rail */
  .ft-tabs-frame {
    inset-inline-start: var(--ft-rail-width) !important;
    width: auto !important;
  }
  .ft-tab-bar {
    display: none;
  }
}
</style>
