<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from "vue";
import { IonBackButton, IonButtons, IonContent, IonFooter, IonHeader, IonIcon, IonTextarea, IonToolbar } from "@ionic/vue";
import { arrowUp, happyOutline, peopleOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import EmojiPicker from "./EmojiPicker.vue";
import MessageBubble from "./MessageBubble.vue";
import { circle as circleOf, loadCircleMessages, markCircleRead, sendCircleText, type CircleMessage } from "../core";
import { t } from "../i18n";
import { useStickToEnd, type Scrollable } from "../viewport";

// A circle's conversation (2026-09-27): like a chat, with who said what over each bubble that is
// not ours, and what happened (who joined, who left) as a line between them. Texts only: files
// and calls in a circle are not in this version.
const props = withDefaults(defineProps<{ circleId: string; showBack?: boolean }>(), { showBack: false });

const router = useRouter();
const circle = computed(() => circleOf(props.circleId));
const messages = computed(() => circle.value?.messages ?? []);
const draft = ref("");
const emoji = ref(false);

/** Whether this phone may write here now. */
const mayWrite = computed(() => !!circle.value && !circle.value.left && (!circle.value.adminsOnly || circle.value.admin));

/**
 * Whether the app is in front (architecture#21): Android keeps the page mounted behind the home
 * screen, and what arrives there has not been seen. A circle tells nobody when you read, but the
 * list's unread count must stay honest.
 */
const seen = () => document.visibilityState === "visible";

async function show() {
  await loadCircleMessages(props.circleId);
  if (seen()) await markCircleRead(props.circleId);
}
/** Back in front of the user: what arrived meanwhile is read now. */
function onVisible() {
  if (seen()) void markCircleRead(props.circleId);
}

async function send() {
  const text = draft.value.trim();
  if (!text) return;
  draft.value = "";
  emoji.value = false;
  await sendCircleText(props.circleId, text);
}

/** What happened, as a sentence: who did it, and to whom or what. */
function eventText(message: CircleMessage): string {
  const who = message.mine ? t("circle.you") : message.senderName;
  return t(`circle.events.${message.kind}`, { who, what: message.text });
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
  <div v-if="circle" class="ft-thread">
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

    <ion-content ref="content" class="ft-thread__content">
      <p class="ft-thread__note">{{ $t("circle.noHistory") }}</p>
      <template v-for="message in messages" :key="message.id">
        <div v-if="message.kind !== 'text'" class="ft-thread__event" data-test="circle-event">
          <span>{{ eventText(message) }}</span>
        </div>
        <MessageBubble v-else :message="message" :sender="message.senderName" />
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
