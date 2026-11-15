<script setup lang="ts">
import { computed, ref } from "vue";
import { IonButton, IonButtons, IonIcon, IonTitle, IonToolbar } from "@ionic/vue";
import { chatbubblesOutline, closeOutline, contractOutline, mailOutline } from "ionicons/icons";
import { t } from "../i18n";
import { focusButton } from "../gameFullscreen";

const props = withDefaults(defineProps<{ name: string; chat?: boolean; invite?: boolean; unread?: number }>(), { unread: 0 });
const emit = defineEmits<{ exit: []; chat: []; invite: []; close: [] }>();

const chatLabel = computed(() => (props.unread ? `${t("games.showChat")}, ${t("games.newMessage")}` : t("games.showChat")));

const exitButton = ref<InstanceType<typeof IonButton> | null>(null);
function focus() {
  return focusButton(exitButton.value?.$el);
}
defineExpose({ focus });
</script>

<template>
  <ion-toolbar class="ft-fullscreen-bar" data-test="game-fullscreen-bar">
    <ion-buttons slot="start">
      <ion-button ref="exitButton" data-test="game-fullscreen-exit" :aria-label="$t('games.exitFullscreen')" aria-pressed="true" @click="emit('exit')">
        <ion-icon slot="icon-only" :icon="contractOutline" aria-hidden="true" />
      </ion-button>
      <ion-button v-if="chat" class="ft-fullscreen-bar__chat" data-test="game-fullscreen-chat" :aria-label="chatLabel" @click="emit('chat')">
        <ion-icon slot="icon-only" :icon="chatbubblesOutline" aria-hidden="true" />
        <span v-if="unread" class="ft-fullscreen-bar__dot" data-test="game-unread" aria-hidden="true" />
      </ion-button>
    </ion-buttons>
    <ion-title size="small"><span class="ft-title" dir="auto">{{ name }}</span></ion-title>
    <ion-buttons slot="end">
      <ion-button v-if="invite" data-test="game-fullscreen-invite" :aria-label="$t('games.invite')" @click="emit('invite')">
        <ion-icon slot="icon-only" :icon="mailOutline" aria-hidden="true" />
      </ion-button>
      <ion-button data-test="game-fullscreen-close" :aria-label="$t('common.close')" @click="emit('close')">
        <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
      </ion-button>
    </ion-buttons>
  </ion-toolbar>
</template>

<style scoped>
.ft-fullscreen-bar {
  flex: none;
  --background: var(--ion-background-color);
  --border-color: var(--ion-border-color);
  --border-width: 0 0 1px;
}
.ft-fullscreen-bar ion-button {
  min-width: 44px;
  min-height: 44px;
}
.ft-fullscreen-bar__chat {
  position: relative;
}
.ft-fullscreen-bar__dot {
  position: absolute;
  inset-block-start: 8px;
  inset-inline-end: 8px;
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--ion-color-primary);
  box-shadow: 0 0 0 2px var(--ion-background-color);
  pointer-events: none;
}
@media (max-width: 359px) {
  .ft-fullscreen-bar ion-title {
    display: none;
  }
}
</style>
