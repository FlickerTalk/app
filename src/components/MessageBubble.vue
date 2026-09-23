<script setup lang="ts">
import { computed, ref } from "vue";
import { readCode } from "../code";
import { piecesOf } from "../links";
import { openUrl } from "@tauri-apps/plugin-opener";
import { IonIcon } from "@ionic/vue";
import { checkmark, checkmarkDone, documentOutline, downloadOutline, pause, play, timeOutline } from "ionicons/icons";
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

const props = defineProps<{ message: Message; saved?: boolean; folded?: boolean }>();
const emit = defineEmits<{ open: [id: string]; save: [id: string]; actions: [id: string] }>();

const STATUS: Record<string, { icon: string; label: string }> = {
  pending: { icon: timeOutline, label: t("status.pending") },
  sent: { icon: checkmark, label: t("status.sent") },
  delivered: { icon: checkmarkDone, label: t("status.delivered") },
  read: { icon: checkmarkDone, label: t("status.read") },
};

const status = computed(() =>
  props.message.mine && props.message.status ? STATUS[props.message.status] : undefined,
);
const file = computed(() => (props.message.kind === "file" ? props.message.file : undefined));
// A message written with fences is code, and the app draws it as such (Ioan, 2026-09-22).
const code = computed(() => (props.message.kind === "file" ? null : readCode(props.message.text ?? "")));
// Web and mail addresses are marked so they can be opened; nothing is fetched to preview them.
const pieces = computed(() => piecesOf(props.message.text ?? ""));

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
const isImage = computed(() => file.value?.mime?.startsWith("image/") ?? false);
const isVoice = computed(() => file.value?.mime?.startsWith("audio/") ?? false);
const isVideo = computed(() => file.value?.mime?.startsWith("video/") ?? false);
const percent = computed(() => Math.round((file.value?.progress ?? 0) * 100));
const fileState = computed(() => {
  if (!file.value || file.value.state === "done") return "";
  if (file.value.state === "failed") return t("status.failed");
  return file.value.state === "paused" ? t("status.paused") : `${percent.value}%`;
});
const moving = computed(() => file.value && file.value.state !== "done" && file.value.state !== "failed");
const failed = computed(() => file.value?.state === "failed");

// A voice message is its own small player: play or pause, how far it is, how long it lasts.
const player = ref<HTMLAudioElement | null>(null);
const playing = ref(false);
const duration = ref(0);
const position = ref(0);
const played = computed(() => (duration.value > 0 ? Math.min(100, (position.value / duration.value) * 100) : 0));
// The waveform is drawn, not measured: bars whose heights come from the message id, so the same
// message always looks the same. The bars up to where it has played are lit.
const BARS = 28;
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

/** `count` bar heights (20–100 %) drawn from `seed`, always the same for the same seed. */
function waveform(seed: string, count: number): number[] {
  let state = 0;
  for (const char of seed) state = (state * 31 + char.charCodeAt(0)) >>> 0;
  return Array.from({ length: count }, () => {
    state = (Math.imul(state, 1_664_525) + 1_013_904_223) >>> 0;
    return 20 + (state % 81);
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
    :class="[message.mine ? 'is-mine' : 'is-theirs', { 'is-pending': message.status === 'pending' }]"
  >
    <div
      class="ft-bubble"
      :class="{ 'is-file': file, 'is-media': file && (isImage || isVideo), 'is-voice': file && isVoice, 'is-folded': folded }"
      data-test="bubble"
      @pointerdown="startPress"
      @pointerup="endPress"
      @pointercancel="endPress"
      @pointerleave="endPress"
    >
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
          <span class="ft-file__name">{{ file.name }}</span>
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
      </div>
      <div v-else-if="code" class="ft-code" data-test="code">
        <span v-if="code.language" class="ft-code__language">{{ code.language }}</span>
        <pre class="ft-code__body"><code>{{ code.code }}</code></pre>
      </div>
      <p v-else class="ft-bubble__text">
        <template v-for="(piece, index) in pieces" :key="index">
          <a
            v-if="piece.kind === 'link'"
            class="ft-bubble__link"
            data-test="link"
            :href="piece.href"
            @click.prevent="openLink(piece.href ?? '')"
            >{{ piece.text }}</a
          >
          <template v-else>{{ piece.text }}</template>
        </template>
      </p>

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
  border-bottom-left-radius: 6px;
}
.is-mine .ft-bubble {
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  border-bottom-right-radius: 6px;
  box-shadow: 0 8px 20px -12px var(--ft-glow);
}
.is-pending .ft-bubble {
  opacity: 0.7;
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

.ft-bubble__text {
  margin: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
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
  border-bottom-right-radius: 6px;
}
.is-theirs .ft-media {
  border-bottom-left-radius: 6px;
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
  left: 10px;
  color: #ffb4a8;
}
.ft-bubble.is-media .ft-bubble__meta {
  right: 10px;
  margin: 0;
  opacity: 1;
}
.ft-media__save {
  position: absolute;
  top: 8px;
  right: 8px;
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
  gap: 2px;
  flex: 1;
  height: 26px;
}
.ft-voice__wave.is-dim {
  opacity: 0.4;
}
.ft-voice__bar {
  display: block;
  flex: 1;
  min-width: 2px;
  border-radius: 2px;
  background: currentColor;
  opacity: 0.35;
}
.ft-voice__bar.is-on {
  opacity: 1;
}
.ft-voice__meta {
  min-width: 34px;
  font-size: 12px;
  text-align: right;
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
