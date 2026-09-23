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
import {
  add,
  appsOutline,
  arrowUp,
  callOutline,
  chevronCollapseOutline,
  chevronExpandOutline,
  closeOutline,
  happyOutline,
  micOutline,
  shareOutline,
  arrowRedoOutline,
  trashOutline,
  videocamOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import EmojiPicker from "./EmojiPicker.vue";
import MessageBubble from "./MessageBubble.vue";
import PluginSheet from "./PluginSheet.vue";
import { installedPlugins } from "../plugins";
import type { PluginView } from "../core";
import {
  chat as chatOf,
  forgetMessage,
  forwardMessage,
  loadMessages,
  markRead,
  openFile,
  pickFiles,
  saveFile,
  sendFile,
  sendPicked,
  sendText,
  shareMessage,
  store,
} from "../core";
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
  emoji.value = false;
  await sendText(props.chatId, text);
}

// §84, issue app#4: the emoji are the app's own, next to the composer.
const emoji = ref(false);

function addEmoji(one: string) {
  draft.value += one;
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

/**
 * The phone's own picker where there is one (§62). On Android the WebView's file input takes the
 * user out of the app with no way back unless they pick something; this returns either way. On a
 * desktop there is no such picker, so the hidden input is used.
 */
async function pick() {
  try {
    for (const file of await pickFiles()) {
      await sendPicked(props.chatId, file);
    }
  } catch {
    picker.value?.click();
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

// A long press asks what to do with a message (Ioan, 2026-09-23): fold it, send it on, hand it to
// another app, or erase it here. Erasing is for good, so it asks once.
const acting = ref("");
const folded = reactive(new Set<string>());
const forwarding = ref(false);
const erasing = ref(false);

/** The other conversations, to send a message on to one of them. */
const others = computed(() => store.chats.filter((one) => one.id !== props.chatId));

function act(id: string) {
  acting.value = id;
  forwarding.value = false;
  erasing.value = false;
}

function closeActions() {
  acting.value = "";
  forwarding.value = false;
  erasing.value = false;
}

function fold() {
  const id = acting.value;
  if (folded.has(id)) folded.delete(id);
  else folded.add(id);
  closeActions();
}

async function share() {
  const id = acting.value;
  closeActions();
  await shareMessage(id).catch(() => {});
}

async function erase() {
  const id = acting.value;
  closeActions();
  folded.delete(id);
  await forgetMessage(id).catch(() => {});
  await loadMessages(props.chatId);
}

async function forwardTo(contact: string) {
  const id = acting.value;
  closeActions();
  await forwardMessage(id, contact).catch(() => {});
}

// Issue app#3: the apps of this phone, each in its own window.
const installed = ref<PluginView[]>([]);
const showApps = ref(false);
const plugin = ref<{ id: string; name: string } | null>(null);

async function loadPlugins() {
  installed.value = await installedPlugins().catch(() => []);
}

function useApp(id: string) {
  const chosen = installed.value.find((one) => one.id === id);
  if (!chosen) return;
  showApps.value = false;
  plugin.value = { id: chosen.id, name: chosen.name };
}

/** A plugin proposes; the user sends (§53). */
function fromPlugin(text: string) {
  draft.value = text;
  plugin.value = null;
}

onMounted(() => {
  void loadPlugins();
  // A permission granted in Settings shows up here as soon as the chat comes back.
  document.addEventListener("visibilitychange", onVisible);
});
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));

function onVisible() {
  if (document.visibilityState === "visible") void loadPlugins();
}

async function save(id: string) {
  await saveFile(id);
  saved.add(id);
}

type Scrollable = { $el?: { scrollToBottom?: (duration: number) => Promise<void> } };
const content = ref<Scrollable | null>(null);

async function scrollToEnd() {
  await nextTick();
  try {
    await content.value?.$el?.scrollToBottom?.(0);
  } catch {
    // A view that cannot scroll yet is not a problem: the conversation shows all the same.
  }
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
          <!-- Issue app#3: the utilities installed on this phone. -->
          <ion-button v-if="installed.length" data-test="apps" :aria-label="$t('plugins.title')" @click="showApps = true">
            <ion-icon slot="icon-only" :icon="appsOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <!-- Issue app#3: each plugin does its thing inside its own window. -->
    <div v-if="plugin" class="ft-app" role="dialog" :aria-label="plugin.name">
      <button type="button" class="ft-app__close" data-test="close-app" :aria-label="$t('common.back')" @click="plugin = null">
        <ion-icon :icon="closeOutline" aria-hidden="true" />
      </button>
      <PluginSheet :plugin="plugin" :contact="chatId" @text="fromPlugin" @done="plugin = null" />
    </div>

    <!-- The apps of this phone; each opens its own window. -->
    <div v-if="showApps" class="ft-apps" role="dialog" :aria-label="$t('plugins.title')" @click.self="showApps = false">
      <ul class="ft-apps__list">
        <li v-for="one in installed" :key="one.id">
          <button type="button" class="ft-apps__item" :data-test="`app-${one.id}`" @click="useApp(one.id)">
            <ion-icon :icon="appsOutline" aria-hidden="true" />
            {{ one.name }}
          </button>
        </li>
      </ul>
    </div>

    <ion-content ref="content" class="ft-thread__content">
      <div class="ft-thread__day"><span>{{ $t("chat.today") }}</span></div>
      <MessageBubble
        v-for="message in messages"
        :key="message.id"
        :message="message"
        :saved="saved.has(message.id)"
        :folded="folded.has(message.id)"
        @open="openFile"
        @save="save"
        @actions="act"
      />

      <div class="ft-thread__end" />
    </ion-content>

    <!-- What can be done with the message that was pressed (§61). -->
    <div v-if="acting" class="ft-actions" data-test="actions" @click.self="closeActions">
      <div v-if="forwarding" class="ft-actions__bar">
        <span class="ft-actions__title">{{ $t("chat.forwardTo") }}</span>
        <button
          v-for="one in others"
          :key="one.id"
          type="button"
          class="ft-actions__to"
          :data-test="`to-${one.id}`"
          @click="forwardTo(one.id)"
        >
          {{ one.name }}
        </button>
        <span v-if="!others.length" class="ft-actions__title">—</span>
      </div>
      <div v-else class="ft-actions__bar">
        <button type="button" class="ft-round ft-round--ghost" data-test="fold" :aria-label="$t(folded.has(acting) ? 'chat.unfold' : 'chat.fold')" @click="fold">
          <ion-icon :icon="folded.has(acting) ? chevronExpandOutline : chevronCollapseOutline" aria-hidden="true" />
        </button>
        <button type="button" class="ft-round ft-round--ghost" data-test="forward" :aria-label="$t('chat.forward')" @click="forwarding = true">
          <ion-icon :icon="arrowRedoOutline" aria-hidden="true" />
        </button>
        <button type="button" class="ft-round ft-round--ghost" data-test="share" :aria-label="$t('chat.share')" @click="share">
          <ion-icon :icon="shareOutline" aria-hidden="true" />
        </button>
        <button
          v-if="!erasing"
          type="button"
          class="ft-round ft-round--ghost ft-actions__danger"
          data-test="delete"
          :aria-label="$t('chat.delete')"
          @click="erasing = true"
        >
          <ion-icon :icon="trashOutline" aria-hidden="true" />
        </button>
        <button v-else type="button" class="ft-actions__sure" data-test="delete-sure" @click="erase">
          {{ $t("chat.deleteSure") }}
        </button>
      </div>
    </div>

    <ion-footer class="ion-no-border">
      <p v-if="voiceError" class="ft-composer__error" role="alert">{{ voiceError }}</p>
      <ion-toolbar class="ft-composer">
        <div class="ft-composer__row">
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('chat.attach')" @click="pick">
            <ion-icon :icon="add" aria-hidden="true" />
          </button>
          <input ref="picker" type="file" multiple hidden @change="attach" />
          <button
            v-if="!recording.active"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-open': emoji }"
            :aria-label="$t('chat.emoji')"
            :aria-pressed="emoji"
            data-test="open-emoji"
            @click="emoji = !emoji"
          >
            <ion-icon :icon="happyOutline" aria-hidden="true" />
          </button>
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
      <emoji-picker v-if="emoji" @pick="addEmoji" />
    </ion-footer>
  </div>
</template>

<style scoped>
.ft-actions {
  position: fixed;
  inset: 0;
  z-index: 20;
  display: grid;
  place-items: end center;
  padding: var(--ft-space-4);
  padding-bottom: calc(var(--ft-space-4) + 72px);
  background: rgba(0, 0, 0, 0.25);
}
.ft-actions__bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 8px;
  border-radius: 18px;
  background: var(--ft-surface);
  box-shadow: 0 18px 40px -20px rgba(0, 0, 0, 0.5);
}
.ft-actions__title {
  padding: 0 8px;
  color: var(--ft-muted);
  font-size: 13px;
}
.ft-actions__to {
  appearance: none;
  border: 1px solid var(--ft-border);
  background: transparent;
  color: inherit;
  font: inherit;
  border-radius: 14px;
  padding: 8px 12px;
  cursor: pointer;
}
.ft-actions__danger {
  color: var(--ion-color-danger);
}
.ft-actions__sure {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--ion-color-danger);
  font: inherit;
  font-weight: 600;
  padding: 0 10px;
  cursor: pointer;
}

.ft-round--ghost.is-open {
  color: var(--ft-accent);
}

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
.ft-app {
  position: fixed;
  inset: 0;
  z-index: 25;
  overflow-y: auto;
  background: var(--ft-bg);
}
.ft-app__close {
  position: sticky;
  top: 0;
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  margin: var(--ft-space-2);
  border: 0;
  border-radius: 50%;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font-size: 20px;
  cursor: pointer;
}

.ft-apps {
  position: fixed;
  inset: 0;
  z-index: 20;
  display: grid;
  place-items: end center;
  padding: var(--ft-space-4);
  background: rgba(0, 0, 0, 0.35);
}
.ft-apps__list {
  width: min(100%, 420px);
  margin: 0;
  padding: var(--ft-space-2);
  list-style: none;
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.3);
}
.ft-apps__item {
  display: flex;
  align-items: center;
  gap: var(--ft-space-3);
  width: 100%;
  padding: 14px 16px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: var(--ft-text);
  font: inherit;
  font-size: 16px;
  text-align: left;
  cursor: pointer;
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
