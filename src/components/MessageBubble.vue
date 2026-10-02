<script setup lang="ts">
import { computed, ref } from "vue";
import { readCode } from "../code";
import { formatSize } from "../core";
import { gameIdFromText, isGame } from "../games";
import { mapsLink, piecesOf, type Place } from "../links";
import { installed, offered } from "../plugins";
import { openUrl } from "@tauri-apps/plugin-opener";
import { IonButton, IonIcon } from "@ionic/vue";
import { alertCircleOutline, gameControllerOutline, checkmark, checkmarkDone, documentOutline, downloadOutline, pause, play, refreshOutline, timeOutline } from "ionicons/icons";
import { t } from "../i18n";

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
}

/**
 * `sender`: in a circle, who said it; shown over a bubble that is not ours (2026-09-27).
 * `games`: where games are played (a conversation, on a phone that has games), an invitation to
 * one gets a way to play it (plan 10.6).
 */
const props = defineProps<{ message: Message; saved?: boolean; folded?: boolean; sender?: string; games?: boolean }>();
const emit = defineEmits<{
  open: [id: string];
  save: [id: string];
  download: [id: string];
  actions: [id: string];
  resend: [id: string];
  play: [id: string];
}>();

const STATUS: Record<string, { icon: string; label: string }> = {
  pending: { icon: timeOutline, label: t("status.pending") },
  sent: { icon: checkmark, label: t("status.sent") },
  delivered: { icon: checkmarkDone, label: t("status.delivered") },
  read: { icon: checkmarkDone, label: t("status.read") },
  // The router refused it (§84): never shown as on its way; it can be sent again.
  unsent: { icon: alertCircleOutline, label: t("status.unsent") },
};

const status = computed(() =>
  props.message.mine && props.message.status ? STATUS[props.message.status] : undefined,
);
const unsent = computed(() => props.message.mine && props.message.status === "unsent");
const file = computed(() => (props.message.kind === "file" ? props.message.file : undefined));
// A message written with fences is code, and the app draws it as such (Ioan, 2026-09-22).
const code = computed(() => (props.message.kind === "file" ? null : readCode(props.message.text ?? "")));
// Web and mail addresses are marked so they can be opened; nothing is fetched to preview them.
const pieces = computed(() => piecesOf(props.message.text ?? ""));

/**
 * The game a text invites to (plan 10.6): only by the exact link of its page, and only a game this
 * phone has or the signed catalogue offers. Anyone can write the text; the button only ever leads
 * to a game of our catalogue. Nothing is fetched here: the conversation hands the catalogue over.
 */
const game = computed(() => {
  if (!props.games || props.message.kind === "file") return undefined;
  const id = gameIdFromText(props.message.text ?? "");
  if (!id) return undefined;
  const here = installed.value.find((one) => one.id === id && isGame(one));
  if (here) return { id, installed: true, name: here.name, size: 0 };
  const listed = offered.value.find((one) => one.id === id && isGame(one));
  return listed ? { id, installed: false, name: listed.name, size: listed.size } : undefined;
});

// A long press asks for what can be done with this message; a tap does nothing of the sort.
const LONG_PRESS = 500;
let pressing: ReturnType<typeof setTimeout> | undefined;

function startPress() {
  pressing = setTimeout(() => emit("actions", props.message.id), LONG_PRESS);
}

function endPress() {
  clearTimeout(pressing);
  pressing = undefined;
}

function openLink(href: string) {
  void Promise.resolve(openUrl(href)).catch(() => undefined);
}

// 2026-10-02: a place (a `geo:` URI) is a card, never a map: no tile is fetched to draw it. A tap
// opens the phone's own maps app there.
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
  return file.value.state === "paused" ? t("status.paused") : `${percent.value}%`;
});
// A4: bigger than what this phone downloads on its own; nothing comes until the user taps.
const waiting = computed(() => file.value?.state === "waiting");
const moving = computed(() => file.value && !["done", "failed", "waiting"].includes(file.value.state));
const failed = computed(() => file.value?.state === "failed");

// A voice message is its own small player: play or pause, how far it is, how long it lasts.
const player = ref<HTMLAudioElement | null>(null);
const playing = ref(false);
const duration = ref(0);
const position = ref(0);
const played = computed(() => (duration.value > 0 ? Math.min(100, (position.value / duration.value) * 100) : 0));
// The waveform is drawn, not measured: bars whose heights come from the message id, so the same
// message always looks the same. The bars up to where it has played are lit.
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

/** `count` bar heights (20–100 %) drawn from `seed`, always the same for the same seed. Each bar
 *  stays near the one before it, so the shape reads as a voice and not as static. */
function waveform(seed: string, count: number): number[] {
  let state = 0;
  for (const char of seed) state = (state * 31 + char.charCodeAt(0)) >>> 0;
  const next = () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return state / 4_294_967_296;
  };
  // A walk that is pulled back towards the middle, so it never flattens against 20 % or 100 %.
  const MIDDLE = 60;
  let height = MIDDLE;
  return Array.from({ length: count }, () => {
    height += (next() - 0.5) * 44 + (MIDDLE - height) * 0.3;
    return Math.round(Math.min(100, Math.max(20, height)));
  });
}

/** Seconds as m:ss, the way a player shows them. */
function formatClock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  return `${Math.floor(whole / 60)}:${String(whole % 60).padStart(2, "0")}`;
}
// Ours is always here; theirs once it has arrived whole and verified.
const usable = computed(() => file.value && file.value.state !== "failed" && (props.message.mine || file.value.state === "done"));

function open() {
  if (usable.value) emit("open", props.message.id);
}
</script>

<template>
  <div
    class="ft-msg"
    :class="[message.mine ? 'is-mine' : 'is-theirs', { 'is-pending': message.status === 'pending', 'is-unsent': unsent }]"
  >
    <!-- Beside the bubble, so it is there for a text, a file or a voice message alike. -->
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
    <div
      class="ft-bubble"
      :class="{ 'is-file': file, 'is-media': file && (isImage || isVideo), 'is-voice': file && isVoice, 'is-folded': folded }"
      data-test="bubble"
      @pointerdown="startPress"
      @pointerup="endPress"
      @pointercancel="endPress"
      @pointerleave="endPress"
    >
      <span v-if="sender && !message.mine" class="ft-bubble__sender" data-test="sender" dir="auto">{{ sender }}</span>
      <!-- Media carry nothing but the medium (Ioan, 2026-09-23): no card, no name, no size. -->
      <span v-if="file && (isImage || isVideo)" class="ft-media" :class="{ 'is-usable': usable }" data-test="media" @click="open">
        <img v-if="file.url && isImage" class="ft-image" :src="file.url" :alt="file.name" loading="lazy" />
        <!-- A video that arrived plays here, from the app's own files (§62). -->
        <video v-else-if="file.url && isVideo" class="ft-video" :src="file.url" controls playsinline preload="metadata" @click.stop />
        <span v-else class="ft-media__blank" aria-hidden="true" />
        <span v-if="moving" class="ft-media__veil">
          <span
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
        <span
          v-if="moving"
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
      <!-- Any other file: icon and name stay, there is nothing else to show it by. -->
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
        <span>{{ message.time }}</span>
        <ion-icon
          v-if="status"
          :icon="status.icon"
          :class="`is-${message.status}`"
          role="img"
          :aria-label="status.label"
        />
      </span>
    </div>
    <!-- Outside the bubble, so the fold does not hide the sign that it is folded. -->
    <span v-if="folded" class="ft-fold" data-test="folded" :aria-label="t('chat.folded')">⌄</span>
  </div>
</template>

<style scoped>
/* In a circle, who said it, over the text; the colour of a name, not of a message. */
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

/* Folded, a long message takes a few lines and fades out; nothing of it is lost. */
.ft-bubble.is-folded {
  max-height: 4.8em;
  overflow: hidden;
  -webkit-mask-image: linear-gradient(to bottom, #000 60%, transparent);
  mask-image: linear-gradient(to bottom, #000 60%, transparent);
}

.ft-bubble {
  max-width: min(78%, 520px);
  padding: 9px 12px 6px;
  border-radius: var(--ft-radius-bubble);
  font-size: var(--ft-font-body);
  line-height: 1.35;
}
.is-theirs .ft-bubble {
  background: var(--ft-surface-2);
  color: var(--ft-text);
  /* The tail: the outer bottom corner, at the start of the line (the right in Arabic). */
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
}

/* Plan 10.6: the way to play a game an invitation is for, under its text: Ionic's button in one of
   Ionic's named colours (the owner's rule, 2026-10-02): `primary` on the other one's bubble, `light`
   (the surface, mapped in variables.css) on this phone's, whose bubble is the primary itself. */
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

/* A place (2026-10-02): a card with the pin, the word and how far off it may be; no map. */
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
/* A message is a paragraph of its own: it lines up with its own direction, not the app's. */
.ft-bubble__text,
.ft-code__body {
  text-align: start;
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
.ft-bubble__meta ion-icon {
  font-size: 15px;
}
.ft-bubble__meta ion-icon.is-read {
  opacity: 1;
  filter: drop-shadow(0 0 4px var(--ft-glow));
}

/* A medium fills its bubble: no frame, no card. The time sits over its corner. */
.ft-bubble.is-media {
  position: relative;
  padding: 0;
  background: none;
  box-shadow: none;
  max-width: min(86%, 520px);
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
/* A4: a file that waits for the user shows its size and a way to ask for it. */
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

/* A voice message is only its player. */
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
  /* On whatever the bubble's colour is: a tint of the text, never a fixed white. */
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
