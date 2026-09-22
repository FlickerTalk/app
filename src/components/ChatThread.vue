<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
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
import { add, arrowUp, callOutline, micOutline, trashOutline, videocamOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import MessageBubble from "./MessageBubble.vue";
import { chat as chatOf, loadMessages, markRead, openFile, saveFile, sendFile, sendText } from "../core";
import { cancelRecording, recording, startRecording, stopRecording } from "../recorder";
import { t } from "../i18n";

const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean }>(), { showBack: false });

const chat = computed(() => chatOf(props.chatId));
const messages = computed(() => chat.value?.messages ?? []);
const draft = ref("");
const router = useRouter();

// Plan §38: what is on screen has been read; new messages arriving while it is open too.
async function show() {
  await loadMessages(props.chatId);
  await markRead(props.chatId);
}

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  draft.value = "";
  await sendText(props.chatId, text);
}

// §62: files go straight to the contact; the picker is the system's.
const picker = ref<HTMLInputElement | null>(null);

async function attach(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  input.value = "";
  for (const file of files) {
    await sendFile(props.chatId, file);
  }
}

// Voice messages: recorded here, sent as an audio file, straight to the contact (§62).
const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;
const elapsed = computed(() => {
  const seconds = Math.max(0, Math.floor((now.value - recording.startedAt) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
});

const voiceError = ref("");

async function record() {
  voiceError.value = "";
  const started = await startRecording();
  if (started !== "recording") {
    voiceError.value = started === "unsupported" ? t("chat.cannotRecordHere") : t("chat.cannotRecord");
    return;
  }
  now.value = Date.now();
  ticking = setInterval(() => (now.value = Date.now()), 500);
}

async function sendVoice() {
  clearInterval(ticking);
  const voice = await stopRecording();
  if (voice) await sendFile(props.chatId, voice);
}

function discardVoice() {
  clearInterval(ticking);
  cancelRecording();
}

onUnmounted(() => {
  if (recording.active) discardVoice();
});

const saved = reactive(new Set<string>());

async function save(id: string) {
  await saveFile(id);
  saved.add(id);
}

type Scrollable = { $el?: { scrollToBottom?: (duration: number) => Promise<void> } };
const content = ref<Scrollable | null>(null);

async function scrollToEnd() {
  await nextTick();
  await content.value?.$el?.scrollToBottom?.(0);
}

onMounted(async () => {
  await show();
  await scrollToEnd();
});
watch(
  () => props.chatId,
  async () => {
    await show();
    await scrollToEnd();
  },
);
// Unconditional: the list's unread count may still be stale when a new message shows up, and
// the core does nothing when there is nothing to mark.
watch(
  () => messages.value.length,
  async () => {
    await markRead(props.chatId);
    await scrollToEnd();
  },
);
</script>

<template>
  <div v-if="chat" class="ft-thread">
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
      <MessageBubble
        v-for="message in messages"
        :key="message.id"
        :message="message"
        :saved="saved.has(message.id)"
        @open="openFile"
        @save="save"
      />
      <div class="ft-thread__end" />
    </ion-content>

    <ion-footer class="ion-no-border">
      <p v-if="voiceError" class="ft-composer__error" role="alert">{{ voiceError }}</p>
      <ion-toolbar class="ft-composer">
        <div class="ft-composer__row">
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('chat.attach')" @click="picker?.click()">
            <ion-icon :icon="add" aria-hidden="true" />
          </button>
          <input ref="picker" type="file" multiple hidden @change="attach" />
          <div v-if="recording.active" class="ft-recording" data-test="recording">
            <span class="ft-recording__dot" aria-hidden="true" />
            <span class="ft-recording__time">{{ elapsed }}</span>
            <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('chat.discardVoice')" @click="discardVoice">
              <ion-icon :icon="trashOutline" aria-hidden="true" />
            </button>
          </div>
          <ion-textarea
            v-else
            v-model="draft"
            class="ft-composer__input"
            :auto-grow="true"
            :rows="1"
            :placeholder="$t('chat.message')"
            :aria-label="$t('chat.message')"
          />
          <button
            v-if="recording.active"
            type="button"
            class="ft-round ft-round--send"
            :aria-label="$t('chat.sendVoice')"
            @click="sendVoice"
          >
            <ion-icon :icon="arrowUp" aria-hidden="true" />
          </button>
          <button v-else-if="draft.trim()" type="button" class="ft-round ft-round--send" :aria-label="$t('chat.send')" @click="send">
            <ion-icon :icon="arrowUp" aria-hidden="true" />
          </button>
          <button v-else type="button" class="ft-round ft-round--send" :aria-label="$t('chat.record')" @click="record">
            <ion-icon :icon="micOutline" aria-hidden="true" />
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

.ft-recording {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 10px;
  min-height: 44px;
  padding: 0 6px 0 14px;
  border-radius: 22px;
  background: var(--ft-surface-2);
}
.ft-recording__dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: var(--ion-color-danger);
  animation: ft-recording-blink 1s ease-in-out infinite;
}
.ft-recording__time {
  flex: 1;
  font-variant-numeric: tabular-nums;
}
@keyframes ft-recording-blink {
  50% {
    opacity: 0.25;
  }
}

.ft-composer__error {
  margin: 0;
  padding: 6px 16px;
  color: var(--ion-color-danger);
  font-size: 13px;
  text-align: center;
}
</style>
