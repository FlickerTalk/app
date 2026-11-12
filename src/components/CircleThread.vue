<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { IonBackButton, IonButtons, IonChip, IonContent, IonFooter, IonHeader, IonIcon, IonLabel, IonTextarea, IonToolbar } from "@ionic/vue";
import { arrowUp, extensionPuzzleOutline, happyOutline, lockClosedOutline, peopleOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import EmojiPicker from "./EmojiPicker.vue";
import HookFrames from "./HookFrames.vue";
import MessageBubble from "./MessageBubble.vue";
import PermissionAsk from "./PermissionAsk.vue";
import { closeOnBackWhile } from "../back";
import { useChatHooks } from "../chatHooks";
import {
  circle as circleOf,
  circleHome,
  grantPlugin,
  loadCircleMessages,
  markCircleRead,
  PREMIUM_PAGE,
  sendCircleText,
  type ChatMessage,
  type CircleMessage,
  type PluginView,
} from "../core";
import { t } from "../i18n";
import { missingPermission, withPermission, type PermissionNeed } from "../permissions";
import { hookIcon, hookLabel, isLocked, pluginIcon, pluginImage, pluginName, refreshPlugins } from "../plugins";
import { useStickToEnd, type Scrollable } from "../viewport";

const props = withDefaults(defineProps<{ circleId: string; showBack?: boolean; active?: boolean }>(), {
  showBack: false,
  active: true,
});

const router = useRouter();
const circle = computed(() => circleOf(props.circleId));
const messages = computed(() => circle.value?.messages ?? []);
const draft = ref("");
const emoji = ref(false);

const mayWrite = computed(() => !!circle.value && !circle.value.left && (!circle.value.adminsOnly || circle.value.admin));

closeOnBackWhile(() => props.active && emoji.value && mayWrite.value, () => (emoji.value = false));

// Android keeps the page mounted behind the home screen, that isn't seen
const seen = () => document.visibilityState === "visible";

async function show() {
  await loadCircleMessages(props.circleId);
  if (seen()) await markCircleRead(props.circleId);
}
function onVisible() {
  if (!seen()) return;
  void markCircleRead(props.circleId);
  hooks.catchUp();
}

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  const to = props.circleId;
  draft.value = "";
  emoji.value = false;
  const out = await hooks.outgoing(text);
  const sent: unknown = await sendCircleText(to, out.text);
  if (out.original) await hooks.keepOriginal(typeof sent === "string" ? sent : undefined, out.original);
}

const texts = computed<ChatMessage[]>(() =>
  messages.value
    .filter((one) => one.kind === "text")
    .map((one) => ({ id: one.id, mine: one.mine, text: one.text, time: one.time, sentAt: 0, status: one.status, notes: one.notes })),
);
const hooks = useChatHooks({
  conversation: () => props.circleId,
  messages: () => texts.value,
  active: () => props.active && seen(),
});
const messageHook = computed(() => {
  const tool = hooks.messageTool.value;
  if (!tool || !hooks.offersHook.value) return undefined;
  return { label: hookLabel(tool, "message"), icon: isLocked(tool) ? lockClosedOutline : hookIcon(tool, "message") };
});
const translatingIcon = computed(() => (hooks.messageTool.value ? hookIcon(hooks.messageTool.value, "message") : extensionPuzzleOutline));

async function runMessageHook(id: string) {
  const message = texts.value.find((one) => one.id === id);
  let chosen = hooks.messageTool.value;
  if (!message || !chosen) return;
  if (isLocked(chosen)) return void router.push(PREMIUM_PAGE);
  if (!chosen.granted.messages) chosen = await allowedToRead(chosen);
  if (chosen) await hooks.noteMessage(chosen, message);
}

const needing = ref<{ tool: PluginView; need: PermissionNeed } | null>(null);
function answerNeed(allowed: boolean) {
  const asked = needing.value;
  needing.value = null;
  asked?.need.answer(allowed);
}
closeOnBackWhile(() => props.active && Boolean(needing.value), () => answerNeed(false));

function allowedToRead(chosen: PluginView): Promise<PluginView | undefined> {
  const line = missingPermission(chosen, "messages");
  if (!line) return Promise.resolve(undefined);
  return new Promise((resolve) => {
    let answered = false;
    needing.value = {
      tool: chosen,
      need: {
        ...line,
        answer: async (allowed) => {
          if (answered) return;
          answered = true;
          if (!allowed) return resolve(undefined);
          try {
            await grantPlugin(chosen.id, withPermission(chosen, "messages", true));
            const now = (await refreshPlugins()).find((one) => one.id === chosen.id);
            resolve(now?.granted.messages ? now : undefined);
          } catch {
            resolve(undefined);
          }
        },
      },
    };
  });
}

// my own events get first-person sentences, "you" as another's subject only reads in English
function eventText(message: CircleMessage): string {
  if (message.mine) return t(`circle.events.${message.kind}ByMe`, { what: message.text });
  return t(`circle.events.${message.kind}`, { who: message.senderName, what: message.text });
}

const content = ref<Scrollable | null>(null);
useStickToEnd(content);

async function scrollToEnd() {
  await nextTick();
  try {
    await content.value?.$el?.scrollToBottom?.(0);
  } catch {
    // fine if it can't scroll yet
  }
}

onMounted(async () => {
  document.addEventListener("visibilitychange", onVisible);
  await show();
  await scrollToEnd();
});
watch(
  () => props.circleId,
  async () => {
    await show();
    await scrollToEnd();
  },
);
watch(
  () => messages.value.length,
  async () => {
    if (seen()) await markCircleRead(props.circleId);
    await scrollToEnd();
  },
);
onUnmounted(() => document.removeEventListener("visibilitychange", onVisible));
</script>

<template>
  <template v-if="circle">
    <ion-header class="ion-no-border">
      <ion-toolbar class="ft-thread__bar">
        <ion-buttons v-if="showBack" slot="start">
          <ion-back-button default-href="/tabs/chats" text="" :aria-label="$t('common.back')" />
        </ion-buttons>
        <button
          type="button"
          class="ft-peer"
          :class="{ 'has-back': showBack }"
          data-test="circle-peer"
          :aria-label="$t('circle.settings')"
          @click="router.push(`/circle/${circle.id}/info`)"
        >
          <Avatar :name="circle.name" :hue="circle.hue" :size="38" />
          <span class="ft-peer__text">
            <span class="ft-peer__name" dir="auto">{{ circle.name }}</span>
            <span class="ft-peer__status">
              <ion-icon :icon="peopleOutline" aria-hidden="true" />
              {{ $t("circle.members", { count: circle.members.length }) }}
            </span>
          </span>
        </button>
      </ion-toolbar>
    </ion-header>

    <HookFrames :hooks="hooks" :contact="circle.id" :session="circleHome(circle.id).session" :active="active" />

    <ion-content ref="content" class="ft-thread__content">
      <div v-if="hooks.translating.value" slot="fixed" class="ft-translating">
        <ion-chip data-test="translating" @click="router.push(`/circle/${circle.id}/info`)">
          <ion-icon :icon="translatingIcon" aria-hidden="true" />
          <ion-label>{{ $t("chat.translating", { language: hooks.translating.value }) }}</ion-label>
        </ion-chip>
      </div>
      <p class="ft-thread__note">{{ $t("circle.noHistory") }}</p>
      <template v-for="message in messages" :key="message.id">
        <div v-if="message.kind !== 'text'" class="ft-thread__event" data-test="circle-event">
          <span>{{ eventText(message) }}</span>
        </div>
        <MessageBubble
          v-else
          :message="message"
          :sender="message.senderName"
          :hook="messageHook"
          :noting="hooks.noting.get(message.id)"
          :language="hooks.languageName.value"
          @hook="runMessageHook"
        />
      </template>
      <div class="ft-thread__end" />
    </ion-content>

    <ion-footer class="ion-no-border">
      <p v-if="circle.left" class="ft-composer__note" data-test="circle-left">{{ $t("circle.left") }}</p>
      <p v-else-if="!mayWrite" class="ft-composer__note" data-test="circle-read-only">{{ $t("circle.readOnly") }}</p>
      <div v-else class="ft-composer">
        <div class="ft-composer__row">
          <button
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
          <ion-textarea
            v-model="draft"
            class="ft-composer__input"
            :auto-grow="true"
            :rows="1"
            :placeholder="$t('circle.message')"
            :aria-label="$t('circle.message')"
          />
          <button type="button" class="ft-round ft-round--send" data-test="circle-send" :disabled="!draft.trim()" :aria-label="$t('chat.send')" @click="send">
            <ion-icon :icon="arrowUp" aria-hidden="true" />
          </button>
        </div>
      </div>
      <emoji-picker v-if="emoji && mayWrite" @pick="draft += $event" />
    </ion-footer>

    <PermissionAsk
      :open="Boolean(needing)"
      :name="needing ? pluginName(needing.tool) : ''"
      :permission="needing?.need"
      :icon="needing ? pluginIcon(needing.tool) : undefined"
      :image="needing ? pluginImage(needing.tool) : undefined"
      @allow="answerNeed(true)"
      @cancel="answerNeed(false)"
    />
  </template>
</template>

<style scoped>
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
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-thread__content {
  --background: var(--ft-bg);
}
.ft-thread__note,
.ft-thread__event {
  display: flex;
  justify-content: center;
  margin: 0;
  padding: var(--ft-space-2) var(--ft-space-4);
  text-align: center;
}
.ft-thread__note {
  padding-top: var(--ft-space-4);
  color: var(--ft-muted);
  font-size: 12px;
}
.ft-thread__event span {
  padding: 3px 10px;
  border-radius: 999px;
  background: var(--ft-surface-2);
  color: var(--ft-muted);
  font-size: 12px;
}
.ft-thread__end {
  height: var(--ft-space-3);
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
.ft-composer {
  padding: 6px 8px calc(8px + var(--ion-safe-area-bottom, 0px));
  background: var(--ft-bg);
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
.ft-composer__note {
  margin: 0;
  padding: 12px 16px calc(12px + var(--ion-safe-area-bottom, 0px));
  color: var(--ft-muted);
  font-size: 13px;
  text-align: center;
}
.ft-round--ghost.is-open {
  color: var(--ft-accent);
}
.ft-round--send:disabled {
  opacity: 0.5;
}
</style>
