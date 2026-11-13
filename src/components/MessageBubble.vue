<script setup lang="ts">
import { computed, ref } from "vue";
import { readCode } from "../code";
import { clock as clockOf, formatSize } from "../core";
import { gameIdFromText, isGame } from "../games";
import { mapsLink, piecesOf, type Place } from "../links";
import { isAppLink, openInApp } from "../opened";
import { hookIcon, hookLabel, installed, offered, pluginName } from "../plugins";
import { openUrl } from "@tauri-apps/plugin-opener";
import { IonButton, IonIcon, IonSpinner } from "@ionic/vue";
import { alertCircleOutline, gameControllerOutline, checkmark, checkmarkDone, checkmarkDoneCircle, documentOutline, downloadOutline, pause, pinOutline, play, refreshOutline, timeOutline } from "ionicons/icons";
import { i18n, t } from "../i18n";

interface TransferredFile {
  name: string;
  size: string;
  progress: number;
  state: string;
  mime?: string;
  url?: string;
}

export interface Message {
  id: string;
  mine: boolean;
  time: string;
  text?: string;
  status?: string;
  kind?: string;
  file?: TransferredFile;
  quote?: { id: string; text: string; mine: boolean; kind: "text" | "file" | "gone" };
  reactions?: { mine?: string; theirs?: string };
  pinned?: boolean;
  edited?: boolean;
  deleted?: boolean;
  scheduledFor?: number;
  updatePending?: boolean;
  notes?: { plugin: string; text: string }[];
}

const props = withDefaults(
  defineProps<{
    message: Message;
    saved?: boolean;
    folded?: boolean;
    sender?: string;
    contactName?: string;
    games?: boolean;
    hook?: { label: string; icon: string };
    noting?: string;
    language?: string;
    connected?: boolean;
  }>(),
  { connected: true },
);
const emit = defineEmits<{
  open: [id: string];
  save: [id: string];
  download: [id: string];
  actions: [id: string];
  resend: [id: string];
  play: [id: string];
  jump: [id: string];
  hook: [id: string];
}>();

const STATUS: Record<string, { icon: string; label: string }> = {
  pending: { icon: timeOutline, label: t("status.pending") },
  sent: { icon: checkmark, label: t("status.sent") },
  delivered: { icon: checkmarkDone, label: t("status.delivered") },
  // read gets its own icon, a glow alone didn't show on Mono's bubble
  read: { icon: checkmarkDoneCircle, label: t("status.read") },
  // refused by the router, never shown as on its way
  unsent: { icon: alertCircleOutline, label: t("status.unsent") },
};

// only from a tool still installed, the list may not have heard of a removal yet
const note = computed(() => {
  if (props.message.kind === "file" || props.message.deleted) return undefined;
  const hook = props.message.mine ? "composer" : "message";
  for (const one of props.message.notes ?? []) {
    const tool = installed.value.find((plugin) => plugin.id === one.plugin && plugin.hooks?.[hook]);
    if (tool) return { text: one.text, icon: hookIcon(tool, hook), label: hookLabel(tool, "message") || props.hook?.label || "" };
  }
  return undefined;
});
const ownText = ref(false);
const shownText = computed(() => (note.value && !ownText.value ? note.value.text : (props.message.text ?? "")));
const offersHook = computed(() => !props.message.mine && props.message.kind !== "file" && !props.message.deleted && Boolean(props.hook) && !note.value);
const sentIn = computed(() => [props.language ? t("chat.sentIn", { language: props.language }) : "", t("chat.view")].filter(Boolean).join(" · "));
const notingLabel = computed(() => {
  const tool = props.noting ? installed.value.find((one) => one.id === props.noting) : undefined;
  return tool ? t("chat.hookWorking", { plugin: pluginName(tool) }) : "";
});

// show the clock until the other phone has the edit, not the old ticks
const shownStatus = computed(() => (props.message.updatePending ? "pending" : props.message.status));
const status = computed(() =>
  props.message.mine && shownStatus.value ? STATUS[shownStatus.value] : undefined,
);
const unsent = computed(() => props.message.mine && props.message.status === "unsent");
const whenLater = computed(() => {
  const at = props.message.scheduledFor;
  if (!at) return "";
  const date = new Date(at);
  if (new Date().toDateString() === date.toDateString()) return clockOf(at);
  const day = new Intl.DateTimeFormat(i18n.global.locale.value, { day: "numeric", month: "short" }).format(date);
  return `${day} ${clockOf(at)}`;
});
const file = computed(() => (props.message.kind === "file" ? props.message.file : undefined));
const code = computed(() => (props.message.kind === "file" ? null : readCode(shownText.value)));
// links are never fetched for a preview
const pieces = computed(() => piecesOf(shownText.value));

// only the exact link to a game of our signed catalogue gets a button, anyone can write the text
const game = computed(() => {
  if (!props.games || props.message.kind === "file") return undefined;
  const id = gameIdFromText(props.message.text ?? "");
  if (!id) return undefined;
  const here = installed.value.find((one) => one.id === id && isGame(one));
  if (here) return { id, installed: true, name: pluginName(here), size: 0 };
  const listed = offered.value.find((one) => one.id === id && isGame(one));
  return listed ? { id, installed: false, name: pluginName(listed), size: listed.size } : undefined;
});

const LONG_PRESS = 500;
let pressing: ReturnType<typeof setTimeout> | undefined;
let longPressed = false;

function startPress() {
  longPressed = false;
  pressing = setTimeout(longPress, LONG_PRESS);
}

function longPress() {
  endPress();
  if (longPressed) return;
  longPressed = true;
  emit("actions", props.message.id);
}

// Android's WebView cancels the pointer on a long press on a link and then sends this
function contextMenu(event: Event) {
  event.preventDefault();
  longPress();
}

function tapQuote(id: string) {
  if (longPressed) return;
  emit("jump", id);
}

function endPress() {
  clearTimeout(pressing);
  pressing = undefined;
}

function openLink(href: string) {
  if (longPressed) return;
  // our own /add and /move links stay in the app and only fill a field
  if (isAppLink(href)) {
    void openInApp(href).then((handled) => handled || openOutside(href));
    return;
  }
  openOutside(href);
}

function openOutside(href: string) {
  void Promise.resolve(openUrl(href)).catch(() => undefined);
}

function accuracyOf(place: Place): string {
  return place.accuracy === null ? "" : t("chat.accuracy", { metres: Math.round(place.accuracy) });
}

function placeLabel(place: Place): string {
  return t("chat.openLocation", { place: [t("chat.location"), accuracyOf(place)].filter(Boolean).join(" ") });
}

function openPlace(place: Place) {
  openLink(mapsLink(place, navigator.userAgent));
}
const isImage = computed(() => file.value?.mime?.startsWith("image/") ?? false);
const isVoice = computed(() => file.value?.mime?.startsWith("audio/") ?? false);
const isVideo = computed(() => file.value?.mime?.startsWith("video/") ?? false);
const percent = computed(() => Math.round((file.value?.progress ?? 0) * 100));
const fileState = computed(() => {
  if (!file.value || file.value.state === "done") return "";
  if (file.value.state === "failed") return t("status.failed");
  if (file.value.state === "waiting") return `${t("status.waiting")} · ${file.value.size}`;
  return paused.value ? t("status.paused") : `${percent.value}%`;
});
const waiting = computed(() => file.value?.state === "waiting");
const moving = computed(() => file.value && !["done", "failed", "waiting"].includes(file.value.state));
// files only travel over the direct connection
const paused = computed(() => Boolean(moving.value) && !props.connected);
const failed = computed(() => file.value?.state === "failed");

const player = ref<HTMLAudioElement | null>(null);
const playing = ref(false);
const duration = ref(0);
const position = ref(0);
const played = computed(() => (duration.value > 0 ? Math.min(100, (position.value / duration.value) * 100) : 0));
// not measured, the bar heights come from the message id
const BARS = 18;
const bars = computed(() => waveform(props.message.id, BARS));
const lit = computed(() => Math.round((played.value / 100) * BARS));
const clock = computed(() => formatClock(playing.value || position.value > 0 ? position.value : duration.value));

function onMetadata(event: Event) {
  const seconds = (event.target as HTMLAudioElement).duration;
  duration.value = Number.isFinite(seconds) ? seconds : 0;
}

function onTime(event: Event) {
  position.value = (event.target as HTMLAudioElement).currentTime;
}

function toggle() {
  const audio = player.value;
  if (!audio) return;
  if (playing.value) audio.pause();
  else void Promise.resolve(audio.play()).catch(() => undefined);
}

function waveform(seed: string, count: number): number[] {
  let state = 0;
  for (const char of seed) state = (state * 31 + char.charCodeAt(0)) >>> 0;
  const next = () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return state / 4_294_967_296;
  };
  const MIDDLE = 60;
  let height = MIDDLE;
  return Array.from({ length: count }, () => {
    height += (next() - 0.5) * 44 + (MIDDLE - height) * 0.3;
    return Math.round(Math.min(100, Math.max(20, height)));
  });
}

function formatClock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
const usable = computed(() => file.value && file.value.state !== "failed" && (props.message.mine || file.value.state === "done"));

function open() {
  if (usable.value) emit("open", props.message.id);
}
</script>

<template>
  <div
    class="ft-msg"
    :class="[message.mine ? 'is-mine' : 'is-theirs', { 'is-pending': message.status === 'pending', 'is-unsent': unsent }]"
    :data-message="message.id"
  >
    <button
      v-if="unsent"
      type="button"
      class="ft-resend"
      data-test="resend"
      :aria-label="t('chat.resend')"
      @click.stop="emit('resend', message.id)"
    >
      <ion-icon :icon="refreshOutline" aria-hidden="true" />
    </button>
    <div class="ft-msg__column" :class="{ 'is-media': file && (isImage || isVideo) }">
    <div
      class="ft-bubble"
      :class="{ 'is-file': file, 'is-media': file && (isImage || isVideo), 'is-voice': file && isVoice, 'is-folded': folded }"
      data-test="bubble"
      @pointerdown="startPress"
      @pointerup="endPress"
      @pointercancel="endPress"
      @pointerleave="endPress"
      @contextmenu="contextMenu"
    >
      <span v-if="sender && !message.mine" class="ft-bubble__sender" data-test="sender" dir="auto">{{ sender }}</span>
      <button
        v-if="message.quote"
        type="button"
        class="ft-quote"
        :class="{ 'is-theirs': !message.quote.mine, 'is-gone': message.quote.kind === 'gone' }"
        data-test="quote"
        :aria-label="t('chat.reply')"
        @click.stop="tapQuote(message.quote.id)"
      >
        <span class="ft-quote__who">{{ message.quote.kind === 'gone' ? '' : message.quote.mine ? t('chat.you') : (contactName ?? sender ?? t('chat.them')) }}</span>
        <span class="ft-quote__text" dir="auto">
          <ion-icon v-if="message.quote.kind === 'file'" :icon="documentOutline" aria-hidden="true" />
          {{ message.quote.kind === 'gone' ? t('chat.quoteGone') : message.quote.text }}
        </span>
      </button>
      <p v-if="message.deleted" class="ft-bubble__text ft-bubble__gone" data-test="deleted">{{ t("chat.deleted") }}</p>
      <span v-else-if="file && (isImage || isVideo)" class="ft-media" :class="{ 'is-usable': usable }" data-test="media" @click="open">
        <img v-if="file.url && isImage" class="ft-image" :src="file.url" :alt="file.name" loading="lazy" />
        <!-- without #t Android's WebView shows black until the video plays -->
        <video v-else-if="file.url && isVideo" class="ft-video" :src="`${file.url}#t=0.001`" controls playsinline preload="metadata" @click.stop />
        <span v-else class="ft-media__blank" aria-hidden="true" />
        <span v-if="moving" class="ft-media__veil">
          <span v-if="paused" class="ft-media__paused">{{ t("status.paused") }}</span>
          <span
            v-else
            class="ft-ring"
            role="progressbar"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-valuenow="percent"
            :aria-label="`${file.name} transfer`"
            :style="{ '--p': `${percent}%` }"
          >
            <span>{{ percent }}%</span>
          </span>
        </span>
        <span v-else-if="failed" class="ft-media__failed">{{ t("status.failed") }}</span>
        <button
          v-else-if="waiting"
          type="button"
          class="ft-download"
          data-test="download"
          :aria-label="t('chat.download')"
          @click.stop="emit('download', message.id)"
        >
          <ion-icon :icon="downloadOutline" aria-hidden="true" />
          <span>{{ file.size }}</span>
        </button>
        <button
          v-if="usable"
          type="button"
          class="ft-media__save"
          :aria-label="saved ? t('chat.saved') : t('chat.save')"
          @click.stop="emit('save', message.id)"
        >
          <ion-icon :icon="saved ? checkmark : downloadOutline" aria-hidden="true" />
        </button>
      </span>
      <div v-else-if="file && isVoice" class="ft-voice" data-test="voice">
        <audio
          v-if="file.url"
          ref="player"
          :src="file.url"
          preload="metadata"
          @play="playing = true"
          @pause="playing = false"
          @ended="playing = false"
          @loadedmetadata="onMetadata"
          @timeupdate="onTime"
        />
        <button
          v-if="!failed"
          type="button"
          class="ft-voice__play"
          :disabled="!usable"
          :aria-label="playing ? t('chat.pause') : t('chat.play')"
          @click.stop="toggle"
        >
          <ion-icon :icon="playing ? pause : play" aria-hidden="true" />
        </button>
        <span class="ft-voice__wave" :class="{ 'is-dim': moving || failed }" aria-hidden="true">
          <i
            v-for="(height, index) in bars"
            :key="index"
            data-test="bar"
            class="ft-voice__bar"
            :class="{ 'is-on': index < lit }"
            :style="{ height: `${height}%` }"
          />
        </span>
        <span v-if="paused" class="ft-voice__meta">{{ t("status.paused") }}</span>
        <span
          v-else-if="moving"
          class="ft-voice__meta"
          role="progressbar"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="percent"
          :aria-label="`${file.name} transfer`"
          >{{ percent }}%</span
        >
        <span v-else-if="failed" class="ft-voice__meta ft-voice__failed">{{ t("status.failed") }}</span>
        <button
          v-else-if="waiting"
          type="button"
          class="ft-download ft-download--inline"
          data-test="download"
          :aria-label="t('chat.download')"
          @click.stop="emit('download', message.id)"
        >
          <ion-icon :icon="downloadOutline" aria-hidden="true" />
          <span>{{ file.size }}</span>
        </button>
        <span v-else class="ft-voice__meta">{{ clock }}</span>
        <button
          v-if="usable"
          type="button"
          class="ft-file__save"
          :aria-label="saved ? t('chat.saved') : t('chat.save')"
          @click.stop="emit('save', message.id)"
        >
          <ion-icon :icon="saved ? checkmark : downloadOutline" aria-hidden="true" />
        </button>
      </div>
      <div v-else-if="file" class="ft-file" :class="{ 'is-usable': usable }" data-test="file" @click="open">
        <span class="ft-file__icon"><ion-icon :icon="documentOutline" aria-hidden="true" /></span>
        <span class="ft-file__body">
          <span class="ft-file__name" dir="auto">{{ file.name }}</span>
          <span v-if="fileState" class="ft-file__meta">{{ fileState }}</span>
          <span
            v-if="moving"
            class="ft-progress"
            role="progressbar"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-valuenow="percent"
            :aria-label="`${file.name} transfer`"
          >
            <span class="ft-progress__bar" :style="{ width: `${percent}%` }" />
          </span>
        </span>
        <button
          v-if="usable"
          type="button"
          class="ft-file__save"
          :aria-label="saved ? t('chat.saved') : t('chat.save')"
          @click.stop="emit('save', message.id)"
        >
          <ion-icon :icon="saved ? checkmark : downloadOutline" aria-hidden="true" />
        </button>
        <button
          v-else-if="waiting"
          type="button"
          class="ft-file__save"
          data-test="download"
          :aria-label="t('chat.download')"
          @click.stop="emit('download', message.id)"
        >
          <ion-icon :icon="downloadOutline" aria-hidden="true" />
        </button>
      </div>
      <div v-else-if="code" class="ft-code" data-test="code">
        <span v-if="code.language" class="ft-code__language">{{ code.language }}</span>
        <pre class="ft-code__body" dir="auto"><code>{{ code.code }}</code></pre>
      </div>
      <p v-else class="ft-bubble__text" dir="auto">
        <template v-for="(piece, index) in pieces" :key="index">
          <a
            v-if="piece.kind === 'link'"
            class="ft-bubble__link"
            data-test="link"
            :href="piece.href"
            draggable="false"
            @click.prevent="openLink(piece.href ?? '')"
            >{{ piece.text }}</a
          >
          <button
            v-else-if="piece.kind === 'place' && piece.place"
            type="button"
            class="ft-place"
            data-test="place"
            :aria-label="placeLabel(piece.place)"
            @click.stop="openPlace(piece.place)"
          >
            <span class="ft-place__pin" aria-hidden="true">📍</span>
            <span class="ft-place__label" aria-hidden="true">{{ t("chat.location") }}</span>
            <span v-if="accuracyOf(piece.place)" class="ft-place__accuracy" aria-hidden="true">{{ accuracyOf(piece.place) }}</span>
          </button>
          <template v-else>{{ piece.text }}</template>
        </template>
      </p>
      <ion-button
        v-if="noting && hook && !message.mine && !note"
        fill="clear"
        size="small"
        class="ft-bubble__hook"
        :disabled="true"
        data-test="noting"
        :aria-label="notingLabel"
        @pointerdown.stop
      >
        <ion-spinner slot="start" name="dots" class="ft-bubble__hook-spinner" aria-hidden="true" />
        {{ hook.label }}
      </ion-button>
      <ion-button v-else-if="offersHook && hook" fill="clear" size="small" class="ft-bubble__hook" data-test="translate" @pointerdown.stop @click.stop="emit('hook', message.id)">
        <ion-icon slot="start" :icon="hook.icon" aria-hidden="true" />
        {{ hook.label }}
      </ion-button>
      <template v-else-if="note">
        <ion-button v-if="ownText && !message.mine" fill="clear" size="small" class="ft-bubble__hook" data-test="translate" @pointerdown.stop @click.stop="ownText = false">
          <ion-icon slot="start" :icon="note.icon" aria-hidden="true" />
          {{ note.label }}
        </ion-button>
        <ion-button v-else-if="!ownText && message.mine" fill="clear" size="small" class="ft-bubble__hook" data-test="sent-in" @pointerdown.stop @click.stop="ownText = true">
          <ion-icon slot="start" :icon="note.icon" aria-hidden="true" />
          {{ sentIn }}
        </ion-button>
        <ion-button v-else fill="clear" size="small" class="ft-bubble__hook" data-test="show-original" @pointerdown.stop @click.stop="ownText = !ownText">
          <ion-icon slot="start" :icon="note.icon" aria-hidden="true" />
          {{ t("chat.showOriginal") }}
        </ion-button>
      </template>
      <ion-button
        v-if="game"
        expand="block"
        :color="message.mine ? 'light' : 'primary'"
        class="ft-play-game"
        data-test="play-game"
        @pointerdown.stop
        @click.stop="emit('play', game.id)"
      >
        <ion-icon slot="start" :icon="gameControllerOutline" aria-hidden="true" />
        <span class="ft-play-game__label">{{ t("games.play") }}</span>
        <span v-if="!game.installed" class="ft-play-game__meta">{{ game.name }} · {{ formatSize(game.size) }}</span>
      </ion-button>

      <span class="ft-bubble__meta">
        <ion-icon v-if="message.pinned" :icon="pinOutline" class="ft-bubble__pin" role="img" :aria-label="t('chat.pinned')" data-test="pinned" />
        <span v-if="message.edited && !message.deleted" class="ft-bubble__edited" data-test="edited">{{ t("chat.edited") }}</span>
        <span v-if="message.scheduledFor" class="ft-bubble__later" data-test="scheduled">{{ t("chat.scheduledFor", { time: whenLater }) }}</span>
        <span v-else>{{ message.time }}</span>
        <ion-icon
          v-if="status && !message.scheduledFor"
          :icon="status.icon"
          :class="`is-${shownStatus}`"
          role="img"
          :aria-label="status.label"
        />
      </span>
    </div>
    <span v-if="message.reactions?.mine || message.reactions?.theirs" class="ft-reactions" data-test="reactions">
      <span v-if="message.reactions?.theirs" class="ft-reactions__one is-theirs" data-test="reaction-theirs">{{ message.reactions.theirs }}</span>
      <span v-if="message.reactions?.mine" class="ft-reactions__one is-mine" data-test="reaction-mine">{{ message.reactions.mine }}</span>
    </span>
    </div>
    <span v-if="folded" class="ft-fold" data-test="folded" :aria-label="t('chat.folded')">⌄</span>
  </div>
</template>

<style scoped>
.ft-bubble__pin {
  font-size: 12px;
  opacity: 0.85;
}
.ft-bubble__later {
  font-style: italic;
}
.ft-bubble__edited {
  font-style: italic;
  opacity: 0.85;
}
.ft-bubble__gone {
  font-style: italic;
  opacity: 0.75;
}
.ft-msg__column {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  min-width: 0;
  max-width: min(78%, 520px);
}
.ft-msg__column.is-media {
  max-width: min(86%, 520px);
}
.is-mine .ft-msg__column {
  align-items: flex-end;
}
.ft-reactions {
  display: flex;
  gap: 6px;
  margin-block-start: -4px;
  padding-inline: 8px;
  font-size: 16px;
  line-height: 1;
}
.ft-reactions__one {
  padding-block: 2px;
}
.ft-quote {
  appearance: none;
  display: block;
  width: 100%;
  margin-block: 0 6px;
  padding-block: 4px;
  padding-inline: 10px 8px;
  border: 0;
  border-inline-start: 3px solid currentColor;
  border-radius: 8px;
  background: rgba(var(--ion-color-medium-rgb), 0.18);
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.ft-quote__who {
  display: block;
  font-size: var(--ft-font-meta);
  font-weight: 600;
  opacity: 0.85;
}
.ft-quote__text {
  display: block;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: var(--ft-font-meta);
  opacity: 0.9;
}
.ft-quote.is-gone .ft-quote__text {
  font-style: italic;
}
.ft-bubble__sender {
  display: block;
  margin-bottom: 2px;
  font-size: 12px;
  font-weight: 600;
  color: var(--ft-accent);
}

.ft-msg {
  display: flex;
  padding: 2px var(--ft-space-4);
}
.ft-msg.is-mine {
  justify-content: flex-end;
}

.ft-fold {
  align-self: flex-end;
  padding: 0 4px;
  font-size: 14px;
  line-height: 1.6;
  opacity: 0.5;
}

.ft-bubble.is-folded {
  max-height: 4.8em;
  overflow: hidden;
  -webkit-mask-image: linear-gradient(to bottom, #000 60%, transparent);
  mask-image: linear-gradient(to bottom, #000 60%, transparent);
}

.ft-bubble {
  max-width: 100%;
  padding: 9px 12px 6px;
  border-radius: var(--ft-radius-bubble);
  font-size: var(--ft-font-body);
  line-height: 1.35;
}
.is-theirs .ft-bubble {
  background: var(--ft-surface-2);
  color: var(--ft-text);
  border-end-start-radius: 6px;
}
.is-mine .ft-bubble {
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  border-end-end-radius: 6px;
  box-shadow: 0 8px 20px -12px var(--ft-glow);
}
.is-pending .ft-bubble {
  opacity: 0.7;
}
.is-unsent .ft-bubble {
  opacity: 0.7;
}
.ft-bubble__meta ion-icon.is-unsent {
  color: var(--ion-color-danger);
}
.ft-resend {
  align-self: center;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  margin-inline-end: 6px;
  border: 0;
  border-radius: 50%;
  background: var(--ft-surface-2);
  color: var(--ion-color-danger);
  font-size: 18px;
  cursor: pointer;
}

.ft-code {
  display: block;
  max-width: min(78vw, 560px);
}
.ft-code__language {
  display: block;
  margin-bottom: 4px;
  color: var(--ft-muted);
  font-size: 12px;
}
.ft-code__body {
  margin: 0;
  padding: 10px 12px;
  border-radius: 12px;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font: 13px/1.5 ui-monospace, "SF Mono", Menlo, monospace;
  overflow-x: auto;
  white-space: pre;
}

.ft-bubble__link {
  color: inherit;
  text-decoration: underline;
  text-underline-offset: 2px;
  -webkit-touch-callout: none;
}

.ft-play-game {
  margin: 8px 0 0;
  --border-radius: 12px;
  --box-shadow: none;
  text-transform: none;
  letter-spacing: normal;
}
.ft-play-game__label {
  font-weight: 600;
}
.ft-play-game__meta {
  margin-inline-start: 8px;
  font-size: 12px;
  font-weight: 400;
  opacity: 0.8;
}

.ft-place {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  padding: 8px 12px;
  border: 1px solid currentColor;
  border-radius: 12px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}

.ft-place__pin {
  font-size: 20px;
  line-height: 1;
}

.ft-place__label {
  font-weight: 600;
}

.ft-place__accuracy {
  opacity: 0.75;
  font-size: 13px;
}

.ft-bubble__text {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.ft-bubble__text,
.ft-code__body {
  text-align: start;
}

.ft-bubble__hook {
  display: flex;
  width: fit-content;
  height: 26px;
  margin: 2px 0 0;
  --padding-start: 0;
  --padding-end: 0;
  --color: currentColor;
  --color-activated: currentColor;
  --color-focused: currentColor;
  --color-hover: currentColor;
  font-size: var(--ft-font-meta);
  font-weight: 600;
  text-transform: none;
  letter-spacing: normal;
}
.ft-bubble__hook::part(native) {
  color: inherit;
}
/* Ionic pulls a start icon out by 0.3em */
.ft-bubble__hook ion-icon[slot="start"],
.ft-bubble__hook ion-spinner[slot="start"] {
  margin-inline-start: 0;
}
.ft-bubble__hook.button-disabled {
  opacity: 0.75;
}
.ft-bubble__hook-spinner {
  width: 18px;
  height: 14px;
  color: inherit;
}

.ft-bubble__meta {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: 4px;
  margin-top: 2px;
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  opacity: 0.7;
}
/* 70 % was under 4.5:1 on Ember and Aurora */
.is-mine .ft-bubble__meta {
  opacity: 0.85;
}
.ft-bubble__meta ion-icon {
  font-size: 15px;
}
.ft-bubble__meta ion-icon.is-read {
  opacity: 1;
  filter: drop-shadow(0 0 4px var(--ft-glow));
}

.ft-bubble.is-media {
  position: relative;
  padding: 0;
  background: none;
  box-shadow: none;
}
.ft-media {
  position: relative;
  display: block;
  overflow: hidden;
  border-radius: var(--ft-radius-bubble);
  box-shadow: 0 8px 20px -12px rgba(0, 0, 0, 0.8);
  background: var(--ft-surface-2);
}
.is-mine .ft-media {
  border-end-end-radius: 6px;
}
.is-theirs .ft-media {
  border-end-start-radius: 6px;
}
.ft-media.is-usable {
  cursor: pointer;
}
.ft-image {
  display: block;
  width: 100%;
  max-height: 320px;
  object-fit: cover;
}
.ft-video {
  display: block;
  width: min(72vw, 420px);
  max-height: 60vh;
  background: #000;
}
.ft-media__blank {
  display: block;
  width: min(60vw, 260px);
  height: 160px;
}
.ft-media__veil {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgba(0, 0, 0, 0.45);
  color: #fff;
}
.ft-ring {
  display: grid;
  place-items: center;
  width: 54px;
  height: 54px;
  border-radius: 50%;
  font-size: 12px;
  font-weight: 600;
  background: conic-gradient(#fff var(--p), rgba(255, 255, 255, 0.25) 0);
}
.ft-ring > span {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.6);
}
.ft-media__paused {
  padding: 4px 10px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 600;
  background: rgba(0, 0, 0, 0.6);
}
.ft-media__failed,
.ft-bubble.is-media .ft-bubble__meta {
  position: absolute;
  bottom: 8px;
  padding: 2px 7px;
  border-radius: 10px;
  font-size: 11px;
  color: #fff;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(6px);
}
.ft-media__failed {
  inset-inline-start: 10px;
  color: #ffb4a8;
}
.ft-download {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  border: 0;
  background: rgba(0, 0, 0, 0.35);
  color: #fff;
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.ft-download ion-icon {
  font-size: 28px;
}
.ft-download--inline {
  position: static;
  flex-direction: row;
  padding: 0 8px;
  border-radius: 12px;
  background: transparent;
  color: var(--ft-accent);
}
.ft-download--inline ion-icon {
  font-size: 18px;
}
.ft-bubble.is-media .ft-bubble__meta {
  inset-inline-end: 10px;
  margin: 0;
  opacity: 1;
}
.ft-media__save {
  position: absolute;
  top: 8px;
  inset-inline-end: 8px;
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border: 0;
  border-radius: 50%;
  font-size: 17px;
  color: #fff;
  background: rgba(0, 0, 0, 0.45);
  backdrop-filter: blur(6px);
}

.ft-bubble.is-voice {
  padding: 8px 10px 6px 8px;
  min-width: 232px;
}
.ft-voice {
  display: flex;
  align-items: center;
  gap: 10px;
}
.ft-voice__play {
  display: grid;
  place-items: center;
  width: 38px;
  height: 38px;
  flex-shrink: 0;
  border: 0;
  border-radius: 50%;
  font-size: 18px;
  color: inherit;
  background: color-mix(in srgb, currentColor 14%, transparent);
}
.ft-voice__play:disabled {
  opacity: 0.5;
}
.is-theirs .ft-voice__play {
  color: var(--ft-accent);
  background: var(--ft-surface);
}
.ft-voice__wave {
  display: flex;
  align-items: center;
  gap: 3px;
  flex: 1;
  height: 26px;
}
.ft-voice__wave.is-dim {
  opacity: 0.4;
}
.ft-voice__bar {
  display: block;
  flex: 1;
  min-width: 3px;
  border-radius: 2px;
  background: currentColor;
  opacity: 0.45;
}
.ft-voice__bar.is-on {
  opacity: 1;
}
.ft-voice__meta {
  min-width: 34px;
  font-size: 12px;
  text-align: end;
  font-variant-numeric: tabular-nums;
  opacity: 0.85;
}
.ft-voice__failed {
  color: #ffb4a8;
}
.is-theirs .ft-voice__failed {
  color: #ff8a70;
}

.ft-file {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 230px;
}
.ft-file__icon {
  display: grid;
  place-items: center;
  width: 42px;
  height: 42px;
  flex-shrink: 0;
  border-radius: 12px;
  font-size: 22px;
  background: rgba(255, 255, 255, 0.2);
}
.is-theirs .ft-file__icon {
  background: var(--ft-surface);
}
.ft-file.is-usable {
  cursor: pointer;
}
.ft-file__save {
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  border: 0;
  border-radius: 50%;
  font-size: 20px;
  color: inherit;
  background: rgba(255, 255, 255, 0.16);
}
.is-theirs .ft-file__save {
  background: var(--ft-surface);
}
.ft-file__body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1;
}
.ft-file__name {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-file__meta {
  font-size: 12px;
  opacity: 0.8;
}

.ft-progress {
  display: block;
  height: 4px;
  margin-top: 6px;
  border-radius: 4px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.18);
}
.ft-progress__bar {
  display: block;
  height: 100%;
  border-radius: inherit;
  background: currentColor;
  transition: width 0.3s ease;
}
</style>
