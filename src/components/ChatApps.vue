<script setup lang="ts">
import { IonContent, IonHeader, IonIcon, IonItem, IonLabel, IonList, IonSegment, IonSegmentButton, IonToolbar } from "@ionic/vue";
import { add, constructOutline, gameControllerOutline } from "ionicons/icons";
import AppTile from "./AppTile.vue";
import { games as installedGames, isLocked, pluginIcon, pluginImage, pluginName, tools } from "../plugins";
import { gameUrl } from "../games";
import type { PluginView } from "../core";
import type { AppsTab } from "../chatApps";

defineProps<{ selected: AppsTab }>();
const emit = defineEmits<{ "update:selected": [tab: AppsTab]; use: [id: string]; play: [id: string]; invite: [game: PluginView]; more: [] }>();

const APPS_TABS = [
  { id: "tools", icon: constructOutline, label: "apps.tools" },
  { id: "games", icon: gameControllerOutline, label: "apps.games" },
] as const;
</script>

<template>
  <ion-header>
    <ion-toolbar>
      <ion-segment :value="selected" @ion-change="emit('update:selected', $event.detail.value === 'games' ? 'games' : 'tools')">
        <ion-segment-button v-for="tab in APPS_TABS" :key="tab.id" :value="tab.id" layout="icon-start" :data-test="`apps-tab-${tab.id}`">
          <ion-icon :icon="tab.icon" aria-hidden="true" />
          <ion-label>{{ $t(tab.label) }}</ion-label>
        </ion-segment-button>
      </ion-segment>
    </ion-toolbar>
  </ion-header>
  <ion-content class="ft-apps-sheet__content">
    <template v-if="selected === 'tools'">
      <div v-if="tools.length" class="ft-app-grid" data-test="apps-sheet-tools">
        <AppTile
          v-for="one in tools"
          :key="one.id"
          :name="pluginName(one)"
          :icon="pluginIcon(one)"
          :image="pluginImage(one)"
          :badge="isLocked(one) ? 'lock' : undefined"
          :data-test="`app-${one.id}`"
          @tap="emit('use', one.id)"
        />
      </div>
      <p v-else class="ft-apps-sheet__note">{{ $t("apps.noTools") }}</p>
    </template>
    <template v-else>
      <div v-if="installedGames.length" class="ft-app-grid" data-test="games-sheet">
        <AppTile
          v-for="one in installedGames"
          :key="one.id"
          :name="pluginName(one)"
          :icon="pluginIcon(one)"
          :image="pluginImage(one)"
          :label="gameUrl(one.id) ? $t('apps.inviteLabel', { name: pluginName(one) }) : undefined"
          :data-test="`game-${one.id}`"
          @tap="emit('play', one.id)"
          @hold="emit('invite', one)"
        />
      </div>
      <p v-else class="ft-apps-sheet__note" data-test="games-sheet">{{ $t("apps.noGames") }}</p>
      <ion-list lines="none">
        <ion-item button :detail="false" lines="none" data-test="more-games-link" @click="emit('more')">
          <ion-icon slot="start" :icon="add" color="primary" aria-hidden="true" />
          <ion-label color="primary">{{ $t("apps.moreGames") }}</ion-label>
        </ion-item>
      </ion-list>
    </template>
  </ion-content>
</template>

<style scoped>
/* Ionic pads a footer for Android's nav bar, not a content */
.ft-apps-sheet__content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-apps-sheet ion-segment-button {
  text-transform: none;
  letter-spacing: 0;
}
.ft-apps-sheet__note {
  margin: 16px 20px 4px;
  text-align: center;
  color: var(--ion-color-medium);
  font-size: 14px;
}
</style>
