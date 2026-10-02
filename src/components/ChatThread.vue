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
  IonItem,
  IonLabel,
  IonList,
  IonModal,
  IonSegment,
  IonSegmentButton,
  IonTextarea,
  IonTitle,
  IonToolbar,
} from "@ionic/vue";
import {
  add,
  appsOutline,
  arrowUp,
  banOutline,
  chatbubblesOutline,
  checkmarkOutline,
  chevronDownOutline,
  chevronUpOutline,
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
  gameControllerOutline,
  mailOutline,
  openOutline,
  trashOutline,
  videocamOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import EmojiPicker from "./EmojiPicker.vue";
import GamePermissions from "./GamePermissions.vue";
import MessageBubble from "./MessageBubble.vue";
import PluginSheet from "./PluginSheet.vue";
import {
  games as installedGames,
  installed,
  offered,
  offeredOnce,
  openersOf,
  refreshPlugins,
  tools,
  viewerOf,
  type HandedFile,
} from "../plugins";
import { gameGrant, gameIdFromText, gameUrl, gamesAvailable, isGame, needsGameGrant } from "../games";
import {
  acceptContact,
  acceptFile,
  chat as chatOf,
  forgetMessage,
  forwardMessage,
  grantPlugin,
  installPlugin,
  loadMessages,
  markRead,
  openFile,
  pickFiles,
  pluginRef,
  readMessageFile,
  resend,
  takePhoto,
  saveFile,
  sendFile,
  sendPicked,
  sendText,
  sessionOf,
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
import { useStickToEnd, watchViewport, type Scrollable } from "../viewport";

/** `play` (plan 10.4): a game to open here at once, from the games tab (`/chat/<id>?play=<id>`). */
const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean; play?: string }>(), {
  showBack: false,
  play: undefined,
});

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

/**
 * A photo taken now with the camera app; nothing happens if the user backs out of it. With no
 * camera (the iOS simulator), or none allowed, the composer says so (2026-09-30).
 */
async function snap() {
  attaching.value = false;
  composerError.value = "";
  let taken: Awaited<ReturnType<typeof takePhoto>>;
  try {
    taken = await takePhoto();
  } catch {
    composerError.value = t("chat.cannotTakePhoto");
    return;
  }
  for (const file of taken) {
    await sendPicked(props.chatId, file);
  }
}

// Voice messages: recorded here, sent as an audio file, straight to the contact (§62).
const now = ref(Date.now());
let ticking: ReturnType<typeof setInterval> | undefined;
const elapsed = computed(() => {
  const seconds = Math.max(0, Math.floor((now.value - recording.startedAt) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
});

const composerError = ref("");

async function record() {
  composerError.value = "";
  const started = await startRecording();
  if (started !== "recording") {
    composerError.value = started === "unsupported" ? t("chat.cannotRecordHere") : t("chat.cannotRecord");
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
/**
 * The plugin on screen, and what it was opened with (2026-09-27): a text, a file, a way back.
 * `game`: a game, played in the conversation's own room rather than in a window over it.
 */
const plugin = ref<{
  id: string;
  name: string;
  sending: Sending;
  live: boolean;
  text?: string;
  file?: HandedFile;
  reference?: string;
  game?: boolean;
} | null>(null);

// Ioan, 2026-10-03 (option A): a game is played inside the conversation, so the two can write to
// each other while they play. It takes the place of the messages, which stay mounted underneath;
// the header (with the voice call) and the composer stay. No video and no files while playing.
const playing = computed(() => Boolean(plugin.value?.game));
/** The thread shown for a moment, with the game still running behind it. */
const peeking = ref(false);
/** How many messages there were when the game opened or the thread was last seen. */
const seenUpTo = ref(0);
/** The latest message from the other one since then: one line above the composer. */
const lastFromThem = computed(() =>
  messages.value
    .slice(seenUpTo.value)
    .filter((one) => !one.mine)
    .at(-1),
);
watch(playing, (now) => {
  peeking.value = false;
  seenUpTo.value = messages.value.length;
  // What arrived while the game covered the thread is read once the thread is back.
  if (!now) void markRead(props.chatId);
});
async function peek() {
  peeking.value = !peeking.value;
  seenUpTo.value = messages.value.length;
  if (peeking.value) {
    await markRead(props.chatId);
    await scrollToEnd();
  }
}
// The keyboard gone (seen on the Samsung, 2026-10-03): the game is shown from its top again, its
// score and status, not left where it was scrolled in the small room the keyboard left.
const gameArea = ref<HTMLElement | null>(null);
let keyboardWasOpen = false;
const unwatchKeyboard = watchViewport({
  before: () => (keyboardWasOpen = document.documentElement.classList.contains("ft-keyboard-open")),
  after: () => {
    if (keyboardWasOpen && !document.documentElement.classList.contains("ft-keyboard-open") && gameArea.value) gameArea.value.scrollTop = 0;
  },
});
onUnmounted(unwatchKeyboard);

/** A file a game made waits in the composer; the `done` that follows it does not end the game. */
let stagedByGame = false;
function pluginDone() {
  if (plugin.value?.game && stagedByGame) {
    stagedByGame = false;
    return;
  }
  plugin.value = null;
}

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

// Plan 10 (app 1.3.0): games, wherever the games tab is (not on iOS). Ioan, 2026-10-03: they are
// a tab of the apps sheet, not a button of their own, so the header keeps three buttons and a name
// has room on a small phone. The apps button is there with a tool, or wherever games can be had,
// so they can be found with nothing installed. A game is opened here, in this conversation, with
// the live channel to the same game on the other phone.
const gamesOn = gamesAvailable();
type AppsTab = "tools" | "games";
const appsTab = ref<AppsTab>("tools");
const APPS_TABS = [
  { id: "tools", icon: appsOutline, label: "plugins.title" },
  { id: "games", icon: gameControllerOutline, label: "tabs.games" },
] as const;
/** The game whose permissions sheet is open (plan decision 11); `size` when it is a download. */
const asking = ref<{ id: string; name: string; size?: number } | null>(null);
closeOnBackWhile(() => Boolean(asking.value), () => (asking.value = null));

/** The apps sheet, on the tools if there are any, otherwise on the games; nothing is remembered. */
function openApps() {
  appsTab.value = !gamesOn || tools.value.length ? "tools" : "games";
  showApps.value = true;
}

function openGame(game: PluginView) {
  plugin.value = { id: game.id, name: game.name, sending: game.granted.send, live: Boolean(game.granted.live), game: true };
}

/** 📨 from the game's own bar: the same invitation, and the game goes on. */
function inviteToPlaying() {
  const game = installed.value.find((one) => one.id === plugin.value?.id);
  if (game) invite(game);
}

/**
 * ▶️ from the games sheet, an invitation or the games tab: a game that has what it needs opens;
 * one that does not asks first; one that is not here yet but is in the signed catalogue asks to
 * be installed. Nothing else opens: never a tool, never something the catalogue does not list.
 */
function playGame(id: string) {
  showApps.value = false;
  composerError.value = "";
  const here = installed.value.find((one) => one.id === id && isGame(one));
  if (here) {
    if (needsGameGrant(here)) asking.value = { id, name: here.name };
    else openGame(here);
    return;
  }
  const listed = offered.value.find((one) => one.id === id && isGame(one));
  if (listed) asking.value = { id, name: listed.name, size: listed.size };
}

async function allowGame() {
  const ask = asking.value;
  asking.value = null;
  if (!ask) return;
  if (ask.size !== undefined) {
    try {
      await installPlugin(ask.id);
    } catch {
      composerError.value = t("games.installFailed");
      return;
    }
  }
  await refreshPlugins();
  let game = installed.value.find((one) => one.id === ask.id && isGame(one));
  if (game && needsGameGrant(game)) {
    await grantPlugin(game.id, gameGrant(game)).catch(() => undefined);
    await refreshPlugins();
    game = installed.value.find((one) => one.id === ask.id && isGame(one));
  }
  if (game && !needsGameGrant(game)) openGame(game);
}

/** 📨 (plan 10.6): an invitation in the composer, with the game's page; the user sends it. */
function invite(game: PluginView) {
  const url = gameUrl(game.id);
  showApps.value = false;
  if (!url) return;
  const text = t("games.inviteText", { game: game.name, url });
  draft.value = draft.value.trim() ? `${draft.value.trimEnd()} ${text}` : text;
}

function moreGames() {
  showApps.value = false;
  void router.push("/tabs/games");
}

// An invitation to a game this phone does not have is matched against the signed catalogue. It is
// read once, and only when such an invitation is here: opening a chat alone fetches nothing.
const pluginsLoaded = ref(false);
const invitedToUnknown = computed(
  () =>
    gamesOn &&
    pluginsLoaded.value &&
    messages.value.some((message) => {
      if (message.kind === "file") return false;
      const id = gameIdFromText(message.text ?? "");
      return Boolean(id) && !installed.value.some((one) => one.id === id && isGame(one));
    }),
);
watch(invitedToUnknown, (now) => {
  if (now) void offeredOnce();
});

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

/** A plugin proposes; the user sends (§53). A game goes on: the composer is in sight below it. */
function fromPlugin(text: string) {
  draft.value = text;
  if (!plugin.value?.game) plugin.value = null;
}

// A2: a file a plugin made with the `propose` permission waits in the composer, like a text it
// proposes: the user sends it, or throws it away.
const staged = ref<PickedFile | null>(null);

function stage(file: PickedFile) {
  staged.value = file;
  if (plugin.value?.game) stagedByGame = true;
  else plugin.value = null;
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

/** §84: a message the router refused goes again, the same message. */
async function resendMessage(id: string) {
  await resend(id).catch(() => {});
}

onMounted(async () => {
  // A tool installed from another window shows up here as soon as the chat comes back.
  document.addEventListener("visibilitychange", onVisible);
  await refreshPlugins();
  pluginsLoaded.value = true;
  if (props.play) playGame(props.play);
});
watch(
  () => props.play,
  (id) => {
    if (id && pluginsLoaded.value) playGame(id);
  },
);
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));

function onVisible() {
  if (document.visibilityState === "visible") void refreshPlugins();
}

async function save(id: string) {
  await saveFile(id);
  saved.add(id);
}

const content = ref<Scrollable | null>(null);
useStickToEnd(content);

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
    // Behind a game only the latest line shows: read once the thread is (§84).
    if (playing.value && !peeking.value) return;
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
          :aria-label="
            $t('chat.contactDetails', {
              name: chat.name,
              status: chat.connected ? $t('chat.direct') : $t('chat.notConnected'),
            })
          "
          @click="router.push(`/contact/${chat.id}`)"
        >
          <Avatar :name="chat.name" :hue="chat.hue" :size="38" :connected="chat.connected" />
          <span class="ft-peer__text">
            <!-- `auto`: a name keeps its own direction, so a Latin name in Arabic is cut at its end. -->
            <span class="ft-peer__name" dir="auto">{{ chat.name }}</span>
            <span class="ft-peer__status" :class="{ 'is-direct': chat.connected }">
              {{ chat.connected ? $t("chat.direct") : $t("chat.notConnected") }}
            </span>
          </span>
        </button>
        <ion-buttons v-if="!isRequest" slot="end">
          <ion-button :aria-label="$t('chat.voiceCall')" @click="router.push(`/call/${chat.id}`)">
            <ion-icon slot="icon-only" :icon="callOutline" aria-hidden="true" />
          </ion-button>
          <!-- Ioan, 2026-10-03: no video while playing. -->
          <ion-button v-if="!playing" :aria-label="$t('chat.videoCall')" @click="router.push(`/call/${chat.id}?video=1`)">
            <ion-icon slot="icon-only" :icon="videocamOutline" aria-hidden="true" />
          </ion-button>
          <!-- Issue app#3: the utilities installed on this phone and, where there are games, the games. -->
          <ion-button v-if="tools.length || gamesOn" data-test="apps" :aria-label="$t('plugins.title')" @click="openApps">
            <ion-icon slot="icon-only" :icon="appsOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <!-- The game room (Ioan, 2026-10-03): the game where the messages are, with a bar of its own:
         a way out, its name and the invitation. The thread stays mounted under it. -->
    <section v-if="plugin && plugin.game" class="ft-room" data-test="game-room" :aria-label="plugin.name">
      <ion-toolbar class="ft-room__bar" data-test="game-bar">
        <ion-buttons slot="start">
          <ion-button data-test="close-game" :aria-label="$t('common.close')" @click="plugin = null">
            <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
        <ion-title size="small">{{ plugin.name }}</ion-title>
        <ion-buttons v-if="gameUrl(plugin.id)" slot="end">
          <ion-button data-test="game-invite" :aria-label="$t('games.invite')" @click="inviteToPlaying">
            <ion-icon slot="icon-only" :icon="mailOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
      <div v-show="!peeking" ref="gameArea" class="ft-room__game" data-test="game-area">
        <PluginSheet
          :plugin="plugin"
          :contact="chatId"
          :sending="plugin.sending"
          :live="plugin.live"
          :session="sessionOf(chatId)"
          @text="fromPlugin"
          @attach="stage"
          @open-chat="(contact) => router.push(`/chat/${contact}`)"
          @done="pluginDone"
        />
      </div>
    </section>

    <!-- Issue app#3: each tool does its thing inside its own window. -->
    <div v-if="plugin && !plugin.game" class="ft-app" role="dialog" :aria-label="plugin.name">
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
        :session="sessionOf(chatId)"
        @text="fromPlugin"
        @attach="stage"
        @open-chat="(contact) => router.push(`/chat/${contact}`)"
        @done="plugin = null"
      />
    </div>

    <!-- The apps of this phone, in Ionic's sheet modal: it rises from the bottom as the apps
         themselves open (Ioan, 2026-10-03). Where there are games, a segment has them. The content
         scrolls at every height (`expand-to-scroll` off): the sheet grows or goes only by its
         handle or its header, so a long list is never stuck. -->
    <ion-modal
      :is-open="showApps"
      class="ft-apps-sheet"
      :aria-label="$t('plugins.title')"
      :breakpoints="[0, 0.5, 1]"
      :initial-breakpoint="0.5"
      :expand-to-scroll="false"
      @did-dismiss="showApps = false"
    >
      <ion-header v-if="gamesOn">
        <ion-toolbar>
          <ion-segment :value="appsTab" @ion-change="appsTab = $event.detail.value === 'games' ? 'games' : 'tools'">
            <ion-segment-button v-for="tab in APPS_TABS" :key="tab.id" :value="tab.id" layout="icon-start" :data-test="`apps-tab-${tab.id}`">
              <ion-icon :icon="tab.icon" aria-hidden="true" />
              <ion-label>{{ $t(tab.label) }}</ion-label>
            </ion-segment-button>
          </ion-segment>
        </ion-toolbar>
      </ion-header>
      <!-- With no segment header, the content keeps clear of the drag handle (iOS draws it over it). -->
      <ion-content class="ft-apps-sheet__content" :class="{ 'ion-padding-top': !gamesOn }">
        <ion-list v-if="!gamesOn || appsTab === 'tools'" data-test="apps-sheet-tools">
          <ion-item v-for="one in tools" :key="one.id" button :detail="false" :data-test="`app-${one.id}`" @click="useApp(one.id)">
            <ion-icon slot="start" :icon="appsOutline" aria-hidden="true" />
            <ion-label class="ion-text-nowrap">{{ one.name }}</ion-label>
          </ion-item>
          <ion-item v-if="!tools.length" lines="none">
            <ion-label color="medium">{{ $t("plugins.none") }}</ion-label>
          </ion-item>
        </ion-list>
        <!-- Plan 10.4–10.6: the games of this phone; each plays here, or is offered to the contact. -->
        <ion-list v-else data-test="games-sheet">
          <ion-item v-for="one in installedGames" :key="one.id" button :detail="false" :data-test="`game-${one.id}`" @click="playGame(one.id)">
            <ion-icon slot="start" :icon="gameControllerOutline" aria-hidden="true" />
            <ion-label class="ion-text-nowrap">{{ one.name }}</ion-label>
            <ion-button
              v-if="gameUrl(one.id)"
              slot="end"
              fill="clear"
              size="default"
              :data-test="`invite-${one.id}`"
              :aria-label="$t('games.invite')"
              @click.stop="invite(one)"
            >
              <ion-icon slot="icon-only" :icon="mailOutline" aria-hidden="true" />
            </ion-button>
          </ion-item>
          <ion-item v-if="!installedGames.length" lines="none">
            <ion-label color="medium">{{ $t("games.none") }}</ion-label>
          </ion-item>
          <ion-item button :detail="false" lines="none" data-test="more-games-link" @click="moreGames">
            <ion-icon slot="start" :icon="add" color="primary" aria-hidden="true" />
            <ion-label color="primary">{{ $t("games.more") }}</ion-label>
          </ion-item>
        </ion-list>
      </ion-content>
    </ion-modal>

    <GamePermissions v-if="asking" :name="asking.name" :size="asking.size" @allow="allowGame" @cancel="asking = null" />

    <ion-content v-show="!playing || peeking" ref="content" class="ft-thread__content">
      <div class="ft-thread__day"><span>{{ $t("chat.today") }}</span></div>
      <MessageBubble
        v-for="message in messages"
        :key="message.id"
        :message="message"
        :saved="saved.has(message.id)"
        :folded="folded.has(message.id)"
        :games="gamesOn"
        @open="tapFile"
        @save="save"
        @download="download"
        @actions="act"
        @resend="resendMessage"
        @play="playGame"
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
      <!-- While playing: the latest line from the other one, and the thread or the game on a tap. -->
      <ion-item
        v-if="playing"
        button
        :detail="false"
        lines="full"
        class="ft-room__strip"
        data-test="game-strip"
        @click="peek"
      >
        <ion-icon slot="start" :icon="peeking ? gameControllerOutline : chatbubblesOutline" aria-hidden="true" />
        <ion-label v-if="peeking" class="ion-text-nowrap">{{ plugin?.name }}</ion-label>
        <ion-label v-else-if="lastFromThem" class="ion-text-nowrap">{{ lastFromThem.kind === "file" ? lastFromThem.file?.name : lastFromThem.text }}</ion-label>
        <ion-label v-else class="ion-text-nowrap" color="medium">{{ $t("games.showChat") }}</ion-label>
        <ion-button
          slot="end"
          fill="clear"
          size="default"
          data-test="game-peek"
          :aria-label="peeking ? $t('games.showGame') : $t('games.showChat')"
          @click.stop="peek"
        >
          <ion-icon slot="icon-only" :icon="peeking ? chevronDownOutline : chevronUpOutline" aria-hidden="true" />
        </ion-button>
      </ion-item>
      <p v-if="composerError" class="ft-composer__error" role="alert">{{ composerError }}</p>
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
          <div v-if="!playing" class="ft-attach">
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
  /* Never wider than the room the toolbar leaves between its buttons: a long name is cut with an
     ellipsis instead of being drawn under them (seen at 360 px with four buttons, 2026-10-03). */
  min-width: 0;
  max-width: 100%;
  padding-inline-start: 8px;
}
/* The avatar keeps its size; the text is what gives way. */
.ft-peer > :first-child {
  flex-shrink: 0;
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
.ft-peer__name,
.ft-peer__status {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-peer__name {
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-peer__status {
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-peer__status.is-direct {
  color: var(--ft-accent);
}

/* The game room: under the header, over the composer, scrolling as the tool window does. */
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



/* The apps sheet scrolls to its last row above Android's navigation bar (edge to edge): Ionic pads
   a footer for it, not a content, so the content's own padding hook takes it, as the composer does. */
.ft-apps-sheet__content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
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
