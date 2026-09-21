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
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import MessageBubble from "./MessageBubble.vue";
import data from "../mock/chats.json";

const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean }>(), { showBack: false });

const chat = computed(() => data.chats.find((candidate) => candidate.id === props.chatId) ?? data.chats[0]);
const draft = ref("");
const router = useRouter();

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
          <ion-back-button default-href="/tabs/chats" text="" :aria-label="$t('common.back')" />
        </ion-buttons>
        <button
          type="button"
          class="ft-peer"
          :class="{ 'has-back': showBack }"
          data-test="peer"
          @click="router.push(`/contact/${chat.id}`)"
        >
          <Avatar :name="chat.name" :hue="chat.hue" :size="38" :connected="chat.connected" />
          <span class="ft-peer__text">
            <span class="ft-peer__name">{{ chat.name }}</span>
            <span class="ft-peer__status" :class="{ 'is-direct': chat.connected }">
              {{ chat.connected ? $t("chat.direct") : $t("chat.notConnected") }}
            </span>
          </span>
        </button>
        <ion-buttons slot="end">
          <ion-button :aria-label="$t('chat.voiceCall')" @click="router.push(`/call/${chat.id}`)">
            <ion-icon slot="icon-only" :icon="callOutline" aria-hidden="true" />
          </ion-button>
          <ion-button :aria-label="$t('chat.videoCall')" @click="router.push(`/call/${chat.id}?video=1`)">
            <ion-icon slot="icon-only" :icon="videocamOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content ref="content" class="ft-thread__content">
      <div class="ft-thread__day"><span>{{ $t("chat.today") }}</span></div>
      <MessageBubble v-for="message in chat.messages" :key="message.id" :message="message" />
      <div class="ft-thread__end" />
    </ion-content>

    <ion-footer class="ion-no-border">
      <ion-toolbar class="ft-composer">
        <div class="ft-composer__row">
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('chat.attach')">
            <ion-icon :icon="add" aria-hidden="true" />
          </button>
          <ion-textarea
            v-model="draft"
            class="ft-composer__input"
            :auto-grow="true"
            :rows="1"
            :placeholder="$t('chat.message')"
            :aria-label="$t('chat.message')"
          />
          <button type="button" class="ft-round ft-round--send" :aria-label="$t('chat.send')" :disabled="!draft.trim()">
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
  appearance: none;
  border: 0;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
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
