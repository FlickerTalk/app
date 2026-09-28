<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonContent,
  IonFabButton,
  IonFabList,
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
  banOutline,
  checkmarkOutline,
  callOutline,
  contractOutline,
  expandOutline,
  cameraOutline,
  closeOutline,
  documentOutline,
  happyOutline,
  imageOutline,
  micOutline,
  shareOutline,
  arrowRedoOutline,
  extensionPuzzleOutline,
  openOutline,
  trashOutline,
  videocamOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import EmojiPicker from "./EmojiPicker.vue";
import MessageBubble from "./MessageBubble.vue";
import PluginSheet from "./PluginSheet.vue";
import { installed, openersOf, refreshPlugins, viewerOf, type HandedFile } from "../plugins";
import {
  acceptContact,
  acceptFile,
  chat as chatOf,
  forgetMessage,
  forwardMessage,
  loadMessages,
  markRead,
  openFile,
  pickFiles,
  pluginRef,
  readMessageFile,
  takePhoto,
  saveFile,
  sendFile,
  sendPicked,
  sendText,
  shareMessage,
  declineContact,
  store,
  type ChatMessage,
  type PickedFile,
  type PluginView,
  type Sending,
} from "../core";
import { cancelRecording, recording, startRecording, stopRecording } from "../recorder";
import { closeOnBackWhile } from "../back";
import { t } from "../i18n";

const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean }>(), { showBack: false });

const chat = computed(() => chatOf(props.chatId));
const messages = computed(() => chat.value?.messages ?? []);

/**
 * A5, as WhatsApp does it (2026-09-28): a stranger who wrote first is answered here, where the yes
 * and the no stand apart and blocking asks once, never from the list, where a small screen made
 * the no easy to hit by mistake. Until then, no composer and no call.
 */
const isRequest = computed(
  () =>
    store.requests.some((one) => one.id === props.chatId) ||
    store.sessions.some((session) => session.requests.some((one) => one.id === props.chatId)),
);
const asksToBlock = ref(false);

async function acceptRequest() {
  await acceptContact(props.chatId);
}

async function blockRequest() {
  asksToBlock.value = false;
  await declineContact(props.chatId);
  await router.push("/tabs/chats");
}
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
 * The phone's own picker where there is one (§62). The «+» unfolds two (Ioan, 2026-09-23): a photo
 * or video comes through the system's sheet over the chat, which closes with a swipe; any other
 * file through the document picker. Never the WebView's own file input, which takes the user out
 * of the app with no way back unless they pick something. On a desktop there is no such picker,
 * so the hidden input is used.
 */
const attaching = ref(false);

async function pick(accept: string) {
  attaching.value = false;
  try {
    for (const file of await pickFiles(accept)) {
      await sendPicked(props.chatId, file);
    }
  } catch {
    picker.value?.click();
  }
}

/** A photo taken now with the camera app; nothing happens if the user backs out of it. */
async function snap() {
  attaching.value = false;
  try {
    for (const file of await takePhoto()) {
      await sendPicked(props.chatId, file);
    }
  } catch {
    // No camera, or not allowed: the option simply does nothing this time.
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
  openingWith.value = false;
}

function closeActions() {
  acting.value = "";
  forwarding.value = false;
  erasing.value = false;
  openingWith.value = false;
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

// Issue app#3: the apps of this phone, each in its own window. The list is the app's, not this
// component's: what Settings installs or removes shows up here without leaving the conversation.
const showApps = ref(false);
/** The plugin on screen, and what it was opened with (2026-09-27): a text, a file, a way back. */
const plugin = ref<{ id: string; name: string; sending: Sending; live: boolean; text?: string; file?: HandedFile; reference?: string } | null>(null);

// Android's back button closes what is open on top, and only that (2026-09-28).
closeOnBackWhile(() => emoji.value, () => (emoji.value = false));
closeOnBackWhile(() => Boolean(acting.value), () => closeActions());
closeOnBackWhile(() => showApps.value, () => (showApps.value = false));
closeOnBackWhile(() => Boolean(plugin.value), () => (plugin.value = null));

function useApp(id: string) {
  const chosen = installed.value.find((one) => one.id === id);
  if (!chosen) return;
  showApps.value = false;
  plugin.value = { id: chosen.id, name: chosen.name, sending: chosen.granted.send, live: Boolean(chosen.granted.live) };
}

// 2026-09-27: "open with": a message goes to a plugin that says it opens its kind. A text only
// to one granted to read what it is handed; a file with its bytes, once it is here whole. The
// plugin also gets a way back to the message (`ref`) that says nothing of the contact.
const openingWith = ref(false);
const openers = computed(() => {
  const message = messages.value.find((one) => one.id === acting.value);
  return message ? openersOf(installed.value, message) : [];
});

async function openWith(id: string) {
  const message = messages.value.find((one) => one.id === acting.value);
  const chosen = installed.value.find((one) => one.id === id);
  closeActions();
  if (!message || !chosen) return;
  await openIn(chosen, message);
}

/** Whether a tap on this file shows it inside the app: the plugin that `views` its kind. */
function viewerFor(message: ChatMessage | undefined): PluginView | undefined {
  return message?.kind === "file" ? viewerOf(installed.value, message.file?.mime || "application/octet-stream") : undefined;
}
const viewable = computed(() => Boolean(viewerFor(messages.value.find((one) => one.id === acting.value))));

/**
 * A tap on a file (document viewer, 2026-09-27): shown here by its viewer when there is one and
 * the bytes can be handed over; otherwise, or when that fails, it goes to another app as before,
 * so the user always gets something.
 */
async function tapFile(id: string) {
  const message = messages.value.find((one) => one.id === id);
  const viewer = viewerFor(message);
  if (message && viewer && (await openIn(viewer, message))) return;
  await openFile(id);
}

/** «Another app» from "open with": the system's viewer, as a tap without a viewer does. */
async function openElsewhere() {
  const id = acting.value;
  closeActions();
  await openFile(id);
}

/** Puts a message in a plugin's window. False when the file could not be handed over. */
async function openIn(chosen: PluginView, message: ChatMessage): Promise<boolean> {
  try {
    // A plugin granted the drive keeps the file by its ref (plan-drive): the bytes never cross the
    // frame, so a file of any size opens with it; the rest are handed the bytes.
    const file =
      message.kind !== "file"
        ? undefined
        : chosen.granted.drive
          ? { name: message.file?.name ?? "", mime: message.file?.mime ?? "application/octet-stream", data: "" }
          : await readMessageFile(message.id);
    const reference = await pluginRef(chosen.id, message.id).catch(() => undefined);
    plugin.value = {
      id: chosen.id,
      name: chosen.name,
      sending: chosen.granted.send,
      live: Boolean(chosen.granted.live),
      text: message.kind === "file" ? undefined : message.text,
      file,
      reference,
    };
    return true;
  } catch {
    // A file not here whole, or too big for a plugin: nothing opens here.
    return false;
  }
}

/** A plugin proposes; the user sends (§53). */
function fromPlugin(text: string) {
  draft.value = text;
  plugin.value = null;
}

// A2: a file a plugin made with the `propose` permission waits in the composer, like a text it
// proposes: the user sends it, or throws it away.
const staged = ref<PickedFile | null>(null);

function stage(file: PickedFile) {
  staged.value = file;
  plugin.value = null;
}

async function sendStaged() {
  const file = staged.value;
  if (!file) return;
  staged.value = null;
  await sendPicked(props.chatId, file);
}

/** A4: a file that waited for the user is asked for. */
async function download(id: string) {
  await acceptFile(id).catch(() => {});
}

onMounted(() => {
  void refreshPlugins();
  // A tool installed from another window shows up here as soon as the chat comes back.
  document.addEventListener("visibilitychange", onVisible);
});
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));

function onVisible() {
  if (document.visibilityState === "visible") void refreshPlugins();
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
        <ion-buttons v-if="!isRequest" slot="end">
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
      <div class="ft-app__bar">
        <button type="button" class="ft-app__close" data-test="close-app" :aria-label="$t('common.back')" @click="plugin = null">
          <ion-icon :icon="closeOutline" aria-hidden="true" />
        </button>
        <span class="ft-app__name">{{ plugin.name }}</span>
      </div>
      <PluginSheet
        :plugin="plugin"
        :contact="chatId"
        :sending="plugin.sending"
        :live="plugin.live"
        :text="plugin.text"
        :file="plugin.file"
        :reference="plugin.reference"
        @text="fromPlugin"
        @attach="stage"
        @open-chat="(contact) => router.push(`/chat/${contact}`)"
        @done="plugin = null"
      />
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
        @open="tapFile"
        @save="save"
        @download="download"
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
      <div v-else-if="openingWith" class="ft-actions__bar">
        <span class="ft-actions__title">{{ $t("chat.openWithPlugin") }}</span>
        <button
          v-for="one in openers"
          :key="one.id"
          type="button"
          class="ft-actions__to"
          :data-test="`open-with-${one.id}`"
          @click="openWith(one.id)"
        >
          {{ one.name }}
        </button>
        <!-- With a viewer, a tap no longer leaves the app: the other apps are still one press away. -->
        <button v-if="viewable" type="button" class="ft-actions__to" data-test="open-elsewhere" @click="openElsewhere">
          <ion-icon :icon="openOutline" aria-hidden="true" /> {{ $t("chat.otherApp") }}
        </button>
      </div>
      <div v-else class="ft-actions__bar">
        <button type="button" class="ft-round ft-round--ghost" data-test="fold" :aria-label="$t(folded.has(acting) ? 'chat.unfold' : 'chat.fold')" @click="fold">
          <ion-icon :icon="folded.has(acting) ? expandOutline : contractOutline" aria-hidden="true" />
        </button>
        <button type="button" class="ft-round ft-round--ghost" data-test="forward" :aria-label="$t('chat.forward')" @click="forwarding = true">
          <ion-icon :icon="arrowRedoOutline" aria-hidden="true" />
        </button>
        <button type="button" class="ft-round ft-round--ghost" data-test="share" :aria-label="$t('chat.share')" @click="share">
          <ion-icon :icon="shareOutline" aria-hidden="true" />
        </button>
        <button
          v-if="openers.length"
          type="button"
          class="ft-round ft-round--ghost"
          data-test="open-with"
          :aria-label="$t('chat.openWithPlugin')"
          @click="openingWith = true"
        >
          <ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" />
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

    <ion-footer v-if="isRequest" class="ion-no-border">
      <div class="ft-request" data-test="request-panel">
        <p class="ft-request__text">{{ $t("requests.inChat", { name: chat.name }) }}</p>
        <p class="ft-request__hint">{{ $t("requests.hint") }}</p>
        <div v-if="!asksToBlock" class="ft-request__actions">
          <button type="button" class="ft-request__no" data-test="request-decline" @click="asksToBlock = true">
            <ion-icon :icon="banOutline" aria-hidden="true" /> {{ $t("requests.block") }}
          </button>
          <button type="button" class="ft-request__yes" data-test="request-accept" @click="acceptRequest">
            <ion-icon :icon="checkmarkOutline" aria-hidden="true" /> {{ $t("requests.accept") }}
          </button>
        </div>
        <div v-else class="ft-request__ask" data-test="request-decline-ask">
          <p class="ft-request__text">{{ $t("requests.blockAsk", { name: chat.name }) }}</p>
          <div class="ft-request__actions">
            <button type="button" class="ft-request__cancel" data-test="request-decline-cancel" @click="asksToBlock = false">
              {{ $t("common.cancel") }}
            </button>
            <button type="button" class="ft-request__no is-sure" data-test="request-decline-confirm" @click="blockRequest">
              <ion-icon :icon="banOutline" aria-hidden="true" /> {{ $t("requests.block") }}
            </button>
          </div>
        </div>
      </div>
    </ion-footer>
    <ion-footer v-else class="ion-no-border">
      <p v-if="voiceError" class="ft-composer__error" role="alert">{{ voiceError }}</p>
      <!-- A2: what a plugin made, waiting for the user to send it or throw it away. -->
      <div v-if="staged" class="ft-staged" data-test="staged">
        <ion-icon :icon="documentOutline" aria-hidden="true" />
        <span class="ft-staged__name">{{ staged.name }}</span>
        <button type="button" class="ft-round ft-round--ghost" data-test="staged-discard" :aria-label="$t('chat.discardAttachment')" @click="staged = null">
          <ion-icon :icon="trashOutline" aria-hidden="true" />
        </button>
        <button type="button" class="ft-round ft-round--send" data-test="staged-send" :aria-label="$t('chat.send')" @click="sendStaged">
          <ion-icon :icon="arrowUp" aria-hidden="true" />
        </button>
      </div>
      <!-- Not an ion-toolbar: that one clips whatever unfolds above it, and the «+» unfolds. -->
      <div class="ft-composer">
        <div class="ft-composer__row">
          <div class="ft-attach">
            <button
              type="button"
              class="ft-round ft-round--ghost"
              :class="{ 'is-open': attaching }"
              :aria-label="$t('chat.attach')"
              :aria-expanded="attaching"
              @click="attaching = !attaching"
            >
              <ion-icon :icon="add" aria-hidden="true" />
            </button>
            <!-- Only what unfolds is a fab list: the «+» itself is the app's own round button. No
                 ion-fab around it, because that one would take the first option for its main button. -->
            <ion-fab-list side="top" class="ft-attach__list" :activated="attaching">
              <ion-fab-button :aria-label="$t('chat.attachMedia')" @click="pick('image/*,video/*')">
                <ion-icon :icon="imageOutline" aria-hidden="true" />
              </ion-fab-button>
              <ion-fab-button :aria-label="$t('chat.attachCamera')" @click="snap">
                <ion-icon :icon="cameraOutline" aria-hidden="true" />
              </ion-fab-button>
              <ion-fab-button :aria-label="$t('chat.attachFile')" @click="pick('')">
                <ion-icon :icon="documentOutline" aria-hidden="true" />
              </ion-fab-button>
            </ion-fab-list>
          </div>
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
      </div>
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
  padding-inline-start: 8px;
}
.ft-peer.has-back {
  padding-inline-start: 0;
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
  /* The window starts under the status bar, or the way out ends up beneath the clock. */
  padding-top: env(safe-area-inset-top);
}
/* A bar of its own, so the way out is always there while the tool scrolls under it. */
.ft-app__bar {
  position: sticky;
  top: 0;
  z-index: 1;
  display: flex;
  align-items: center;
  gap: var(--ft-space-2);
  padding: var(--ft-space-2);
  background: var(--ft-bg);
}
.ft-app__close {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  flex: none;
  border: 0;
  border-radius: 50%;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font-size: 20px;
  cursor: pointer;
}
.ft-app__name {
  font-size: var(--ft-font-title);
  font-weight: 600;
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
  text-align: start;
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
  padding: 6px 8px calc(8px + var(--ion-safe-area-bottom, 0px));
  background: var(--ft-bg);
}
.ft-composer__row {
  display: flex;
  align-items: flex-end;
  gap: 8px;
}
/* The «+» is the row's own button; what it unfolds rises above the composer, over the thread. */
.ft-attach {
  position: relative;
  flex-shrink: 0;
}
.ft-attach__list {
  left: 0;
  z-index: 3;
}
.ft-attach ion-fab-list ion-fab-button {
  --background: var(--ft-surface-2);
  --color: var(--ft-accent);
  --box-shadow: 0 8px 20px -12px rgba(0, 0, 0, 0.8);
  font-size: 20px;
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
/* A5 (2026-09-28): the answer to a stranger, where the composer would be; big, apart targets. */
.ft-request {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 16px calc(14px + env(safe-area-inset-bottom));
  border-top: 1px solid var(--ft-border);
  background: var(--ft-surface);
  text-align: center;
}
.ft-request__text {
  margin: 0;
  font-weight: 600;
}
.ft-request__hint {
  margin: 0;
  font-size: 13px;
  color: var(--ft-muted);
}
.ft-request__actions {
  display: flex;
  justify-content: center;
  gap: 24px;
  margin-top: 6px;
}
.ft-request__yes,
.ft-request__no,
.ft-request__cancel {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 120px;
  min-height: 44px;
  padding: 0 18px;
  border-radius: 999px;
  font-weight: 600;
}
.ft-request__yes {
  border: 0;
  color: var(--ft-on-accent);
  background: var(--ft-accent);
}
.ft-request__no,
.ft-request__cancel {
  border: 1px solid var(--ft-border);
  color: var(--ft-text);
  background: transparent;
}
.ft-request__no.is-sure {
  border-color: var(--ion-color-danger);
  color: #fff;
  background: var(--ion-color-danger);
}
.ft-staged {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 6px 8px 0;
  padding: 6px 6px 6px 14px;
  border-radius: 22px;
  background: var(--ft-surface-2);
}
.ft-staged__name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: 14px;
}
</style>
