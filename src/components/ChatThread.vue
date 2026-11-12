<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from "vue";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonChip,
  IonContent,
  IonFooter,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonModal,
  IonText,
  IonToolbar,
} from "@ionic/vue";
import {
  appsOutline,
  banOutline,
  chatbubblesOutline,
  checkmarkOutline,
  chevronDownOutline,
  chevronUpOutline,
  callOutline,
  extensionPuzzleOutline,
  gameControllerOutline,
  lockClosedOutline,
  pinOutline,
  videocamOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import ChatApps from "./ChatApps.vue";
import ChatComposer from "./ChatComposer.vue";
import ChatSearch from "./ChatSearch.vue";
import GamePermissions from "./GamePermissions.vue";
import PermissionAsk from "./PermissionAsk.vue";
import MessageActions from "./MessageActions.vue";
import MessageBubble from "./MessageBubble.vue";
import PluginWindow from "./PluginWindow.vue";
import HookFrames from "./HookFrames.vue";
import SchedulePicker from "./SchedulePicker.vue";
import { hookIcon, hookLabel, isLocked, pluginIcon, pluginImage, pluginName, pluginsReady, refreshPlugins, refreshPremiumLock } from "../plugins";
import {
  acceptContact,
  acceptFile,
  block,
  chat as chatOf,
  isSupport,
  isTyping,
  loadMessages,
  markRead,
  resend,
  saveFile,
  requestPending,
  sendText,
  sessionOf,
  declineContact,
  editMessage,
} from "../core";
import { useChatHooks } from "../chatHooks";
import { closeOnBackWhile } from "../back";
import { i18n, t } from "../i18n";
import { takeSend } from "../pending-send";
import { dayLabels } from "../days";
import { useStickToEnd, type Scrollable } from "../viewport";
import { useComposer } from "../composer";
import { jumpTo, useChatSearch } from "../chatSearch";
import { usePinnedMessages } from "../pinnedMessages";
import { useVoiceMessage } from "../voiceMessage";
import { usePluginWindow } from "../pluginWindow";
import { useChatApps } from "../chatApps";
import { useMessageActions } from "../messageActions";
import { useOpenWith } from "../openWith";
import { useSendLater } from "../sendLater";

const props = withDefaults(defineProps<{ chatId: string; showBack?: boolean; play?: string; search?: boolean; split?: boolean; active?: boolean }>(), {
  showBack: false,
  play: undefined,
  search: false,
  split: false,
  active: true,
});
const sheetPane = computed(() => (props.split ? "ft-sheet--chat-pane" : "ft-sheet--window"));

const chat = computed(() => chatOf(props.chatId));
const messages = computed(() => chat.value?.messages ?? []);
// "now" is read when the thread changes, not at midnight
const days = computed(() =>
  dayLabels(
    messages.value.map((message) => message.sentAt),
    Date.now(),
    i18n.global.locale.value,
    t("chat.today"),
  ),
);

const isRequest = computed(() => requestPending(props.chatId));
const asksToBlock = ref(false);

async function acceptRequest() {
  await acceptContact(props.chatId);
}

async function blockRequest() {
  asksToBlock.value = false;
  await declineContact(props.chatId);
  await router.push("/tabs/chats");
}
const router = useRouter();

// Android keeps the page mounted behind the home screen, a message there must not send a read receipt
const seen = () => props.active && document.visibilityState === "visible";

async function show() {
  await loadMessages(props.chatId);
  if (seen()) await markRead(props.chatId);
}
function readOnReturn() {
  // behind a game only the latest line shows, read once the thread is back
  if (seen() && !(playing.value && !peeking.value)) void markRead(props.chatId);
}

const composerView = ref<InstanceType<typeof ChatComposer> | null>(null);
const { draft, editing, replying, emoji, attaching, staged, composerError, cancelEdit, addEmoji, attach, pick, snap, sendStaged } = useComposer({
  chatId: () => props.chatId,
  isRequest: () => isRequest.value,
  picker: () => composerView.value?.picker,
});

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  if (editing.value) {
    const id = editing.value.id;
    draft.value = "";
    emoji.value = false;
    editing.value = null;
    await editMessage(id, text).catch(() => {});
    return;
  }
  const to = props.chatId;
  const answering = replying.value?.id;
  draft.value = "";
  emoji.value = false;
  replying.value = null;
  const out = await hooks.outgoing(text);
  const sent = await sendText(to, out.text, answering);
  if (out.original) await hooks.keepOriginal(sent, out.original);
}

const searchPanel = ref<InstanceType<typeof ChatSearch> | null>(null);
const { searching, query, hits, searched, lit, openSearch, closeSearch, typeSearch, visitHit } = useChatSearch({
  chatId: () => props.chatId,
  asked: () => props.search,
  active: () => props.active,
  field: () => searchPanel.value?.searchbar?.$el,
});

const { pinned, pinnedAt, shownPin, visitPin, pinText } = usePinnedMessages(() => messages.value);

const peerStatus = computed(() =>
  isTyping(props.chatId) ? t("chat.typing") : chat.value?.connected ? t("chat.direct") : t("chat.notConnected"),
);

const { recording, elapsed, record, sendVoice, discardVoice } = useVoiceMessage({ chatId: () => props.chatId, error: composerError });

const saved = reactive(new Set<string>());

const pluginView = ref<InstanceType<typeof PluginWindow> | null>(null);
const {
  plugin,
  leaving,
  openPlugin,
  closePlugin,
  pluginClosed,
  playing,
  peeking,
  lastFromThem,
  peek,
  pluginDone,
  openChatFromPlugin,
  needing,
  askingFor,
  askingView,
  answerNeed,
  allowedToRead,
  needFromApp,
  fromPlugin,
  stage,
} = usePluginWindow({
  chatId: () => props.chatId,
  active: () => props.active,
  split: () => props.split,
  messages: () => messages.value,
  sheet: () => pluginView.value?.sheet,
  gameArea: () => pluginView.value?.gameArea,
  scrollToEnd,
  draft,
  staged,
});
defineExpose({ leave: closePlugin, openSearch });

const { showApps, appsTab, asking, toPremium, useApp, openApps, inviteToPlaying, playGame, allowGame, invite, moreGames, loaded } = useChatApps({
  messages: () => messages.value,
  play: () => props.play,
  draft,
  error: composerError,
  plugin,
  openPlugin,
});

const {
  acting,
  folded,
  forwarding,
  erasing,
  openingWith,
  others,
  actingMessage,
  canEdit,
  act,
  closeActions,
  fold,
  share,
  erase,
  forwardTo,
  reactWith,
  togglePin,
  reply,
  edit,
  eraseEverywhere,
} = useMessageActions({ chatId: () => props.chatId, messages: () => messages.value, draft, editing, replying });

const { openers, viewable, openWith, openElsewhere, tapFile } = useOpenWith({
  messages: () => messages.value,
  acting,
  closeActions,
  openPlugin,
  allowedToRead,
  toPremium,
});

closeOnBackWhile(() => props.active && emoji.value, () => (emoji.value = false));
closeOnBackWhile(() => props.active && Boolean(acting.value), () => closeActions());
closeOnBackWhile(() => props.active && showApps.value, () => (showApps.value = false));
closeOnBackWhile(() => props.active && Boolean(plugin.value) && !leaving.value, () => closePlugin());
closeOnBackWhile(() => props.active && Boolean(needing.value), () => answerNeed(false));
closeOnBackWhile(() => props.active && Boolean(asking.value), () => (asking.value = null));

const hooks = useChatHooks({
  conversation: () => props.chatId,
  messages: () => messages.value,
  active: () => seen() && !isRequest.value && !chat.value?.blocked,
});
const messageHook = computed(() => {
  const tool = hooks.messageTool.value;
  if (!tool || !hooks.offersHook.value || isRequest.value) return undefined;
  return { label: hookLabel(tool, "message"), icon: isLocked(tool) ? lockClosedOutline : hookIcon(tool, "message") };
});

const translatingIcon = computed(() => (hooks.messageTool.value ? hookIcon(hooks.messageTool.value, "message") : extensionPuzzleOutline));

async function runMessageHook(id: string) {
  const message = messages.value.find((one) => one.id === id);
  let chosen = hooks.messageTool.value;
  if (!message || !chosen) return;
  if (isLocked(chosen)) return toPremium();
  if (!chosen.granted.messages) chosen = await allowedToRead(chosen);
  if (chosen) await hooks.noteMessage(chosen, message);
}

const { scheduling, sendAt, schedulePresented, soonest, latest, weekStart, canSchedule, openSchedule, pickTime, schedule } = useSendLater({
  chatId: () => props.chatId,
  draft,
  emoji,
  editing,
  replying,
  hooks,
});

watch(
  () => props.active && props.chatId,
  (here) => {
    const proposal = here ? takeSend(here) : null;
    if (proposal?.kind === "text") draft.value = proposal.text;
    else if (proposal) staged.value = proposal.file;
  },
  { immediate: true },
);

async function download(id: string) {
  await acceptFile(id).catch(() => {});
}

async function resendMessage(id: string) {
  await resend(id).catch(() => {});
}

onMounted(async () => {
  document.addEventListener("visibilitychange", onVisible);
  // reading the list again here cost 30–75 ms mid-transition
  await Promise.all([pluginsReady(), refreshPremiumLock()]);
  loaded();
});
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));

function onVisible() {
  if (document.visibilityState !== "visible") return;
  void refreshPlugins();
  void refreshPremiumLock();
  readOnReturn();
  hooks.catchUp();
}
watch(() => props.active, readOnReturn);

async function save(id: string) {
  await saveFile(id);
  saved.add(id);
}

const content = ref<Scrollable | null>(null);
const list = ref<HTMLElement | null>(null);
useStickToEnd(content, list);

async function scrollToEnd() {
  await nextTick();
  try {
    await content.value?.$el?.scrollToBottom?.(0);
  } catch {
    // fine if it can't scroll yet
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
watch(
  () => messages.value.length,
  async () => {
    if (playing.value && !peeking.value) return;
    if (seen()) await markRead(props.chatId);
    await scrollToEnd();
  },
);
</script>

<template>
  <template v-if="chat">
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
          :aria-label="$t('chat.contactDetails', { name: chat.name, status: peerStatus })"
          @click="router.push(`/contact/${chat.id}`)"
        >
          <Avatar :name="chat.name" :hue="chat.hue" :size="38" :connected="chat.connected" />
          <span class="ft-peer__text">
            <!-- dir="auto" so a Latin name in Arabic is cut at its end -->
            <span class="ft-peer__name" dir="auto">{{ chat.name }}</span>
            <span class="ft-peer__status" :class="{ 'is-direct': chat.connected, 'is-typing': isTyping(chat.id) }" data-test="peer-status">
              {{ peerStatus }}
            </span>
          </span>
        </button>
        <ion-buttons v-if="!isRequest" slot="end">
          <ion-button :aria-label="$t('chat.voiceCall')" @click="router.push(`/call/${chat.id}`)">
            <ion-icon slot="icon-only" :icon="callOutline" aria-hidden="true" />
          </ion-button>
          <ion-button v-if="!playing" :aria-label="$t('chat.videoCall')" @click="router.push(`/call/${chat.id}?video=1`)">
            <ion-icon slot="icon-only" :icon="videocamOutline" aria-hidden="true" />
          </ion-button>
          <ion-button data-test="apps" :aria-label="$t('tabs.apps')" @click="openApps">
            <ion-icon slot="icon-only" :icon="appsOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ChatSearch
      v-if="searching"
      ref="searchPanel"
      :query="query"
      :hits="hits"
      :searched="searched"
      :contact-name="chat.name"
      @type="typeSearch"
      @close="closeSearch"
      @visit="visitHit"
    />
    <button v-if="shownPin" type="button" class="ft-pinned" data-test="pinned-strip" :aria-label="$t('chat.pinned')" @click="visitPin">
      <ion-icon :icon="pinOutline" aria-hidden="true" />
      <span class="ft-pinned__text" dir="auto">{{ pinText(shownPin) }}</span>
      <span v-if="pinned.length > 1" class="ft-pinned__count">{{ (pinnedAt % pinned.length) + 1 }}/{{ pinned.length }}</span>
    </button>

    <PluginWindow
      v-if="plugin"
      v-show="!leaving"
      ref="pluginView"
      :plugin="plugin"
      :peeking="peeking"
      :session="sessionOf(plugin.contact)"
      @close="closePlugin"
      @invite="inviteToPlaying"
      @text="fromPlugin"
      @attach="stage"
      @open-chat="openChatFromPlugin"
      @done="pluginDone"
      @closed="pluginClosed"
      @needs="needFromApp"
    />

    <!-- expand-to-scroll off, so a long list never gets stuck -->
    <ion-modal
      :is-open="showApps"
      class="ft-apps-sheet"
      :class="sheetPane"
      :aria-label="$t('tabs.apps')"
      :breakpoints="[0, 0.5, 1]"
      :initial-breakpoint="0.5"
      :expand-to-scroll="false"
      @did-dismiss="showApps = false"
    >
      <ChatApps v-model:selected="appsTab" @use="useApp" @play="playGame" @invite="invite" @more="moreGames" />
    </ion-modal>

    <PermissionAsk
      :class="sheetPane"
      :open="Boolean(needing)"
      :name="askingFor ? pluginName(askingFor) : (plugin?.name ?? '')"
      :permission="needing ?? undefined"
      :icon="askingView && pluginIcon(askingView)"
      :image="askingView && pluginImage(askingView)"
      @allow="answerNeed(true)"
      @cancel="answerNeed(false)"
    />

    <GamePermissions
      :class="sheetPane"
      :open="Boolean(asking)"
      :name="asking?.name ?? ''"
      :size="asking?.size"
      :icon="asking?.icon"
      :image="asking?.image"
      @allow="allowGame"
      @cancel="asking = null"
    />

    <HookFrames :hooks="hooks" :contact="chatId" :session="sessionOf(chatId)" :active="active" :pane-class="sheetPane" />

    <ion-content v-show="!playing || peeking" ref="content" class="ft-thread__content">
      <div v-if="hooks.translating.value && !isRequest" slot="fixed" class="ft-translating">
        <ion-chip data-test="translating" @click="router.push(`/contact/${chatId}`)">
          <ion-icon :icon="translatingIcon" aria-hidden="true" />
          <ion-label>{{ $t("chat.translating", { language: hooks.translating.value }) }}</ion-label>
        </ion-chip>
      </div>
      <div ref="list">
        <ion-text v-if="!messages.length && isSupport(chatId)" color="medium">
          <p class="ft-thread__support" data-test="support-hint">{{ $t("chat.supportHint") }}</p>
        </ion-text>
        <template v-for="(message, index) in messages" :key="message.id">
          <div v-if="days[index]" class="ft-thread__day"><span>{{ days[index] }}</span></div>
          <MessageBubble
            :message="message"
            :class="{ 'is-lit': lit === message.id }"
            :saved="saved.has(message.id)"
            :folded="folded.has(message.id)"
            :contact-name="chat?.name"
            :hook="messageHook"
            :noting="hooks.noting.get(message.id)"
            :language="hooks.languageName.value"
            games
            @open="tapFile"
            @save="save"
            @download="download"
            @actions="act"
            @resend="resendMessage"
            @play="playGame"
            @jump="jumpTo"
            @hook="runMessageHook"
          />
        </template>

        <div class="ft-thread__end" />
      </div>
    </ion-content>

    <MessageActions
      v-if="acting"
      v-model:forwarding="forwarding"
      v-model:opening-with="openingWith"
      v-model:erasing="erasing"
      :message="actingMessage"
      :folded="folded.has(acting)"
      :can-edit="canEdit"
      :request="isRequest"
      :others="others"
      :openers="openers"
      :viewable="viewable"
      @close="closeActions"
      @react="reactWith"
      @edit="edit"
      @pin="togglePin"
      @reply="reply"
      @fold="fold"
      @share="share"
      @erase="erase"
      @erase-everywhere="eraseEverywhere"
      @forward-to="forwardTo"
      @open-with="openWith"
      @open-elsewhere="openElsewhere"
    />

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
    <!-- a message to someone blocked would wait forever -->
    <ion-footer v-else-if="chat.blocked" class="ion-no-border">
      <div class="ft-request" data-test="blocked-panel">
        <p class="ft-request__text">{{ $t("blocked.notice") }}</p>
        <ion-button class="ion-align-self-center" fill="outline" shape="round" data-test="unblock" @click="block(chatId, false)">
          {{ $t("contact.unblock") }}
        </ion-button>
      </div>
    </ion-footer>
    <ion-footer v-else class="ion-no-border">
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
        <ion-label v-else-if="lastFromThem" class="ion-text-nowrap" dir="auto">{{ lastFromThem.kind === "file" ? lastFromThem.file?.name : lastFromThem.text }}</ion-label>
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
      <ChatComposer
        ref="composerView"
        v-model:draft="draft"
        v-model:emoji="emoji"
        v-model:attaching="attaching"
        :error="composerError"
        :staged="staged"
        :editing="editing"
        :replying="replying"
        :contact-name="chat?.name"
        :playing="playing"
        :recording="recording.active"
        :elapsed="elapsed"
        @add-emoji="addEmoji"
        @send="send"
        @send-later="openSchedule"
        @record="record"
        @send-voice="sendVoice"
        @discard-voice="discardVoice"
        @pick="pick"
        @snap="snap"
        @attach="attach"
        @cancel-edit="cancelEdit"
        @cancel-reply="replying = null"
        @discard-staged="staged = null"
        @send-staged="sendStaged"
      />
    </ion-footer>

    <ion-modal
      :is-open="scheduling"
      class="ft-schedule-sheet"
      :breakpoints="[0, 1]"
      :initial-breakpoint="1"
      :aria-label="$t('chat.sendLater')"
      @did-present="schedulePresented = true"
      @did-dismiss="scheduling = false; schedulePresented = false"
    >
      <SchedulePicker
        :presented="schedulePresented"
        :send-at="sendAt"
        :soonest="soonest"
        :latest="latest"
        :week-start="weekStart"
        :can-schedule="canSchedule"
        @pick="pickTime"
        @cancel="scheduling = false"
        @schedule="schedule"
      />
    </ion-modal>
  </template>
</template>

<style scoped>
.ft-schedule-sheet {
  --height: auto;
}
.ft-msg.is-lit :deep(.ft-bubble) {
  outline: 2px solid var(--ion-color-primary);
  outline-offset: 2px;
}
.ft-pinned {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding-block: 6px;
  padding-inline: var(--ft-space-4);
  border: 0;
  border-bottom: 1px solid var(--ion-border-color);
  background: var(--ion-color-light);
  color: var(--ion-text-color);
  font: inherit;
  font-size: var(--ft-font-meta);
  text-align: start;
  cursor: pointer;
}
.ft-pinned ion-icon {
  flex: none;
  color: var(--ion-color-primary);
}
.ft-pinned__text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
.ft-pinned__count {
  flex: none;
  color: var(--ion-color-medium);
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
  max-width: 100%;
  padding-inline-start: 8px;
}
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
.ft-peer__status.is-typing {
  color: var(--ion-color-primary);
  font-style: italic;
}

.ft-thread__content {
  --background: var(--ft-bg);
}
.ft-translating {
  inset-block-start: 4px;
  inset-inline: 0;
  display: flex;
  justify-content: center;
  pointer-events: none;
}
.ft-translating ion-chip {
  pointer-events: auto;
  margin: 0;
  background: var(--ion-color-light);
  color: var(--ion-text-color);
  font-size: var(--ft-font-meta);
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
.ft-thread__support {
  margin: 0;
  padding: var(--ft-space-4) var(--ft-space-4) var(--ft-space-2);
  font-size: 13px;
  text-align: center;
}

.ft-request {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 14px 16px calc(14px + var(--ion-safe-area-bottom, 0px));
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
</style>
