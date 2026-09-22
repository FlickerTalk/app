<script setup lang="ts">
import { computed } from "vue";
import { IonIcon } from "@ionic/vue";
import { checkmark, checkmarkDone, documentOutline, downloadOutline, extensionPuzzleOutline, micOutline, timeOutline } from "ionicons/icons";
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

const props = defineProps<{ message: Message; saved?: boolean; withPlugin?: boolean }>();
const emit = defineEmits<{ open: [id: string]; save: [id: string]; plugin: [id: string] }>();

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
const isImage = computed(() => file.value?.mime?.startsWith("image/") ?? false);
const isVoice = computed(() => file.value?.mime?.startsWith("audio/") ?? false);
const percent = computed(() => Math.round((file.value?.progress ?? 0) * 100));
const fileState = computed(() => {
  if (!file.value || file.value.state === "done") return "";
  if (file.value.state === "failed") return ` · ${t("status.failed")}`;
  return file.value.state === "paused" ? ` · ${t("status.paused")}` : ` · ${percent.value}%`;
});
const moving = computed(() => file.value && file.value.state !== "done" && file.value.state !== "failed");
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
    <div class="ft-bubble" :class="{ 'is-file': file }">
      <img v-if="file?.url && isImage" class="ft-image" :src="file.url" :alt="file.name" loading="lazy" @click="open" />
      <audio v-if="file?.url && isVoice" class="ft-voice" :src="file.url" controls preload="metadata" />
      <div v-if="file" class="ft-file" :class="{ 'is-usable': usable }" data-test="file" @click="open">
        <span class="ft-file__icon"><ion-icon :icon="isVoice ? micOutline : documentOutline" aria-hidden="true" /></span>
        <span class="ft-file__body">
          <span class="ft-file__name">{{ isVoice ? t("chat.voiceMessage") : file.name }}</span>
          <span class="ft-file__meta">{{ file.size }}{{ fileState }}</span>
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
      <p v-else class="ft-bubble__text">{{ message.text }}</p>
      <!-- Issue app#3: hand this message to a plugin, only because the user asked (§53). -->
      <button
        v-if="withPlugin && !file && message.text"
        type="button"
        class="ft-bubble__plugin"
        data-test="plugin"
        :aria-label="t('chat.openWithPlugin')"
        @click.stop="emit('plugin', message.id)"
      >
        <ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" />
      </button>

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

.ft-image {
  display: block;
  width: 100%;
  max-height: 320px;
  margin-bottom: 8px;
  border-radius: calc(var(--ft-radius-bubble) - 6px);
  object-fit: cover;
}

.ft-voice {
  display: block;
  width: min(260px, 100%);
  height: 40px;
  margin-bottom: 6px;
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
