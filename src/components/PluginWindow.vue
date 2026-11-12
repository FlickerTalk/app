<script setup lang="ts">
import { ref } from "vue";
import { IonButton, IonButtons, IonIcon, IonTitle, IonToolbar } from "@ionic/vue";
import { closeOutline, mailOutline } from "ionicons/icons";
import PluginSheet from "./PluginSheet.vue";
import { gameUrl } from "../games";
import type { PickedFile } from "../core";
import type { PermissionNeed } from "../permissions";
import type { OpenPlugin } from "../pluginWindow";

defineProps<{ plugin: OpenPlugin; peeking: boolean; session?: string }>();
const emit = defineEmits<{
  close: [];
  invite: [];
  text: [text: string];
  attach: [file: PickedFile];
  openChat: [contact: string];
  done: [];
  closed: [];
  needs: [need: PermissionNeed];
}>();

const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
const gameArea = ref<HTMLElement | null>(null);
defineExpose({ sheet, gameArea });
</script>

<template>
  <section v-if="plugin.game" class="ft-room" data-test="game-room" :aria-label="plugin.name">
    <ion-toolbar class="ft-room__bar" data-test="game-bar">
      <ion-buttons slot="start">
        <ion-button data-test="close-game" :aria-label="$t('common.close')" @click="emit('close')">
          <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
      <ion-title size="small"><span class="ft-title">{{ plugin.name }}</span></ion-title>
      <ion-buttons v-if="gameUrl(plugin.id)" slot="end">
        <ion-button data-test="game-invite" :aria-label="$t('games.invite')" @click="emit('invite')">
          <ion-icon slot="icon-only" :icon="mailOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
    </ion-toolbar>
    <div v-show="!peeking" ref="gameArea" class="ft-room__game" data-test="game-area">
      <PluginSheet
        ref="sheet"
        :key="plugin.opening"
        :plugin="plugin"
        :contact="plugin.contact"
        :sending="plugin.sending"
        :live="plugin.live"
        :session="session"
        @text="emit('text', $event)"
        @attach="emit('attach', $event)"
        @open-chat="emit('openChat', $event)"
        @done="emit('done')"
        @closed="emit('closed')"
        @needs="emit('needs', $event)"
      />
    </div>
  </section>

  <div v-else class="ft-app" role="dialog" :aria-label="plugin.name">
    <ion-toolbar class="ft-app__bar" data-test="app-bar">
      <ion-buttons slot="start">
        <ion-button data-test="close-app" :aria-label="$t('common.back')" @click="emit('close')">
          <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
      <ion-title size="small"><span class="ft-title">{{ plugin.name }}</span></ion-title>
    </ion-toolbar>
    <div class="ft-app__body">
      <PluginSheet
        ref="sheet"
        :key="plugin.opening"
        :plugin="plugin"
        :contact="plugin.contact"
        :sending="plugin.sending"
        :live="plugin.live"
        :text="plugin.text"
        :file="plugin.file"
        :reference="plugin.reference"
        :session="session"
        fill
        @text="emit('text', $event)"
        @attach="emit('attach', $event)"
        @open-chat="emit('openChat', $event)"
        @done="emit('done')"
        @closed="emit('closed')"
        @needs="emit('needs', $event)"
      />
    </div>
  </div>
</template>

<style scoped>
.ft-room {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-height: 0;
}
.ft-room__game {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding-left: var(--ion-safe-area-left, 0px);
  padding-right: var(--ion-safe-area-right, 0px);
}

.ft-app {
  position: fixed;
  inset: 0;
  z-index: 25;
  display: flex;
  flex-direction: column;
  background: var(--ft-bg);
  padding-top: var(--ion-safe-area-top, 0px);
}
.ft-app__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  padding-bottom: var(--ion-safe-area-bottom, 0px);
  padding-left: var(--ion-safe-area-left, 0px);
  padding-right: var(--ion-safe-area-right, 0px);
}
.ft-app__bar {
  flex: none;
}
</style>
