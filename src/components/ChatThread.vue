<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonContent,
  IonFooter,
  IonHeader,
  IonIcon,
  IonTextarea,
  IonToolbar,
} from "@ionic/vue";
import { add, arrowUp, callOutline, videocamOutline } from "ionicons/icons";
import Avatar from "./Avatar.vue";
import MessageBubble from "./MessageBubble.vue";
import data from "../mock/chats.json";

const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean }>(), { showBack: false });

const chat = computed(() => data.chats.find((candidate) => candidate.id === props.chatId) ?? data.chats[0]);
const draft = ref("");

type Scrollable = { $el?: { scrollToBottom?: (duration: number) => Promise<void> } };
const content = ref<Scrollable | null>(null);

async function scrollToEnd() {
  await nextTick();
  await content.value?.$el?.scrollToBottom?.(0);
}

onMounted(scrollToEnd);
watch(() => props.chatId, scrollToEnd);
</script>

<template>
  <div class="ft-thread">
    <ion-header class="ion-no-border">
      <ion-toolbar class="ft-thread__bar">
        <ion-buttons v-if="showBack" slot="start">
          <ion-back-button default-href="/tabs/chats" text="" aria-label="Back" />
        </ion-buttons>
        <div class="ft-peer" :class="{ 'has-back': showBack }">
          <Avatar :name="chat.name" :hue="chat.hue" :size="38" :connected="chat.connected" />
          <span class="ft-peer__text">
            <span class="ft-peer__name">{{ chat.name }}</span>
            <span class="ft-peer__status" :class="{ 'is-direct': chat.connected }">
              {{ chat.connected ? "Direct" : "Not connected" }}
            </span>
          </span>
        </div>
        <ion-buttons slot="end">
          <ion-button aria-label="Voice call">
            <ion-icon slot="icon-only" :icon="callOutline" aria-hidden="true" />
          </ion-button>
          <ion-button aria-label="Video call">
            <ion-icon slot="icon-only" :icon="videocamOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content ref="content" class="ft-thread__content">
      <div class="ft-thread__day"><span>Today</span></div>
      <MessageBubble v-for="message in chat.messages" :key="message.id" :message="message" />
      <div class="ft-thread__end" />
    </ion-content>

    <ion-footer class="ion-no-border">
      <ion-toolbar class="ft-composer">
        <div class="ft-composer__row">
          <button type="button" class="ft-round ft-round--ghost" aria-label="Attach file">
            <ion-icon :icon="add" aria-hidden="true" />
          </button>
          <ion-textarea
            v-model="draft"
            class="ft-composer__input"
            :auto-grow="true"
            :rows="1"
            placeholder="Message"
            aria-label="Message"
          />
          <button type="button" class="ft-round ft-round--send" aria-label="Send" :disabled="!draft.trim()">
            <ion-icon :icon="arrowUp" aria-hidden="true" />
          </button>
        </div>
      </ion-toolbar>
    </ion-footer>
  </div>
</template>

<style scoped>
.ft-thread {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: var(--ft-bg);
}

.ft-thread__bar {
  --min-height: 60px;
  --padding-start: 8px;
  --padding-end: 6px;
}

.ft-peer {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  padding-left: 8px;
}
.ft-peer.has-back {
  padding-left: 0;
}
.ft-peer__text {
  display: flex;
  flex-direction: column;
  min-width: 0;
  line-height: 1.2;
}
.ft-peer__name {
  font-size: var(--ft-font-title);
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-peer__status {
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-peer__status.is-direct {
  color: var(--ft-accent);
}

.ft-thread__content {
  --background: var(--ft-bg);
}
.ft-thread__day {
  display: flex;
  justify-content: center;
  padding: var(--ft-space-4) 0 var(--ft-space-2);
}
.ft-thread__day span {
  padding: 3px 10px;
  border-radius: 999px;
  background: var(--ft-surface-2);
  color: var(--ft-muted);
  font-size: 12px;
}
.ft-thread__end {
  height: var(--ft-space-3);
}

.ft-composer {
  --background: var(--ft-bg);
  --border-width: 0;
  --padding-top: 6px;
  --padding-bottom: 8px;
  --padding-start: 8px;
  --padding-end: 8px;
}
.ft-composer__row {
  display: flex;
  align-items: flex-end;
  gap: 8px;
}
.ft-composer__input {
  flex: 1;
  min-height: 44px;
  margin: 0;
  overflow: hidden;
  border-radius: 22px;
  font-size: 16px;
  --background: var(--ft-surface-2);
  --color: var(--ft-text);
  --placeholder-color: var(--ft-muted);
  --padding-start: 16px;
  --padding-end: 16px;
  --padding-top: 11px;
  --padding-bottom: 11px;
}
</style>
