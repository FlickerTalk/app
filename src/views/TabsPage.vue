<script setup lang="ts">
import { IonIcon, IonPage, IonRouterOutlet, IonTabBar, IonTabButton, IonTabs } from "@ionic/vue";
import { callOutline, chatbubblesOutline, settingsOutline } from "ionicons/icons";
import NavRail from "../components/NavRail.vue";
import { t } from "../i18n";

const tabs = [
  { tab: "chats", href: "/tabs/chats", label: t("tabs.chats"), icon: chatbubblesOutline },
  { tab: "calls", href: "/tabs/calls", label: t("tabs.calls"), icon: callOutline },
  { tab: "settings", href: "/tabs/settings", label: t("tabs.settings"), icon: settingsOutline },
];
</script>

<template>
  <ion-page>
    <NavRail />
    <!-- IonTabs sets an inline `inset: 0`, so the offset for the rail goes on this frame. -->
    <div class="ft-tabs-frame">
      <ion-tabs>
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
    </div>
  </ion-page>
</template>

<style>
.ft-tabs-frame {
  position: absolute;
  inset: 0;
}

.ft-tab-bar {
  --border: 1px solid var(--ft-border);
  height: calc(56px + env(safe-area-inset-bottom));
  backdrop-filter: blur(16px);
  -webkit-backdrop-filter: blur(16px);
}
.ft-tab-bar ion-tab-button {
  font-size: 13px;
}
.ft-tab-bar ion-tab-button ion-icon {
  font-size: 25px;
}

/* Wide screens: the bottom tab bar becomes the navigation rail. */
@media (min-width: 768px) {
  .ft-tabs-frame {
    inset-inline-start: var(--ft-rail-width);
  }
  .ft-tab-bar {
    display: none;
  }
}
</style>
