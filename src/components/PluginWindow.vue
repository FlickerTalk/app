<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IonButton, IonButtons, IonIcon, IonTitle, IonToolbar } from "@ionic/vue";
import { closeOutline, expandOutline, mailOutline } from "ionicons/icons";
import PluginSheet from "./PluginSheet.vue";
import GameFullscreenBar from "./GameFullscreenBar.vue";
import { gameUrl } from "../games";
import { focusButton, useFullscreenCues } from "../gameFullscreen";
import { useTypingInFrame } from "../frameTyping";
import { t } from "../i18n";
import { isSupport, type PickedFile } from "../core";
import type { PermissionNeed } from "../permissions";
import type { OpenPlugin } from "../pluginWindow";

const props = withDefaults(defineProps<{ plugin: OpenPlugin; peeking: boolean; session?: string; fullscreen?: boolean; unread?: number }>(), {
  session: undefined,
  unread: 0,
});
const emit = defineEmits<{
  close: [];
  invite: [];
  text: [text: string];
  attach: [file: PickedFile];
  openChat: [contact: string];
  done: [];
  closed: [];
  needs: [need: PermissionNeed];
  fullscreen: [on: boolean];
  showChat: [];
}>();

const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
const gameArea = ref<HTMLElement | null>(null);
const typing = useTypingInFrame(
  () => gameArea.value?.querySelector("iframe"),
  () => gameArea.value,
);
defineExpose({ sheet, gameArea, typing });

const invites = computed(() => Boolean(gameUrl(props.plugin.id)) && !isSupport(props.plugin.contact ?? ""));
const bar = ref<InstanceType<typeof GameFullscreenBar> | null>(null);
const expand = ref<InstanceType<typeof IonButton> | null>(null);
const { said, announce } = useFullscreenCues({
  on: () => props.fullscreen,
  inside: () => bar.value,
  outside: () => ({ focus: () => focusButton(expand.value?.$el) }),
  quiet: () => props.peeking,
});
watch(
  () => props.unread,
  (now, before) => {
    if (props.fullscreen && now > before) announce(t("games.newMessage"));
  },
);
</script>

<template>
  <section v-if="plugin.game" class="ft-room" data-test="game-room" :aria-label="plugin.name">
    <ion-toolbar class="ft-room__bar" data-test="game-bar" :inert="fullscreen || undefined">
      <ion-buttons slot="start">
        <ion-button data-test="close-game" :aria-label="$t('common.close')" @click="emit('close')">
          <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
      <ion-title size="small"><span class="ft-title">{{ plugin.name }}</span></ion-title>
      <ion-buttons slot="end">
        <ion-button v-if="invites" data-test="game-invite" :aria-label="$t('games.invite')" @click="emit('invite')">
          <ion-icon slot="icon-only" :icon="mailOutline" aria-hidden="true" />
        </ion-button>
        <ion-button ref="expand" data-test="game-fullscreen" :aria-label="$t('games.enterFullscreen')" aria-pressed="false" @click="emit('fullscreen', true)">
          <ion-icon slot="icon-only" :icon="expandOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
    </ion-toolbar>
    <div
      v-show="!peeking"
      ref="gameArea"
      class="ft-room__game"
      :class="{ 'ft-room__game--fullscreen': fullscreen }"
      data-test="game-area"
    >
      <GameFullscreenBar
        v-if="fullscreen"
        ref="bar"
        :name="plugin.name"
        chat
        :invite="invites"
        :unread="unread"
        @exit="emit('fullscreen', false)"
        @chat="emit('showChat')"
        @invite="emit('invite')"
        @close="emit('close')"
      />
      <PluginSheet
        ref="sheet"
        :key="plugin.opening"
        :plugin="plugin"
        :contact="plugin.contact"
        :sending="plugin.sending"
        :live="plugin.live"
        :session="session"
        :fill="fullscreen"
        @text="emit('text', $event)"
        @attach="emit('attach', $event)"
        @open-chat="emit('openChat', $event)"
        @done="emit('done')"
        @closed="emit('closed')"
        @needs="emit('needs', $event)"
      />
    </div>
    <span class="ft-room__said" role="status" aria-live="polite">{{ said }}</span>
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
.ft-room__game--fullscreen {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 0;
  padding-bottom: var(--ion-safe-area-bottom, 0px);
  background: var(--ion-background-color);
}
.ft-room__game--fullscreen > .ft-fullscreen-bar {
  padding-top: var(--ion-safe-area-top, 0px);
}
.ft-room__game--fullscreen > :deep(.ft-plugin) {
  flex: 1;
  min-height: 0;
  padding-inline: 0;
  padding-left: var(--ion-safe-area-left, 0px);
  padding-right: var(--ion-safe-area-right, 0px);
}
.ft-room__said {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
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
