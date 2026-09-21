<script setup lang="ts">
import { computed } from "vue";
import { IonIcon } from "@ionic/vue";
import { checkmark, checkmarkDone, documentOutline, timeOutline } from "ionicons/icons";

interface TransferredFile {
  name: string;
  size: string;
  progress: number;
  state: string;
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

const props = defineProps<{ message: Message }>();

const STATUS: Record<string, { icon: string; label: string }> = {
  pending: { icon: timeOutline, label: "Waiting for device" },
  sent: { icon: checkmark, label: "Sent" },
  delivered: { icon: checkmarkDone, label: "Delivered" },
  read: { icon: checkmarkDone, label: "Read" },
};

const status = computed(() =>
  props.message.mine && props.message.status ? STATUS[props.message.status] : undefined,
);
const file = computed(() => (props.message.kind === "file" ? props.message.file : undefined));
const percent = computed(() => Math.round((file.value?.progress ?? 0) * 100));
const fileState = computed(() => {
  if (!file.value || file.value.state === "done") return "";
  return file.value.state === "paused" ? " · Paused" : ` · ${percent.value}%`;
});
</script>

<template>
  <div
    class="ft-msg"
    :class="[message.mine ? 'is-mine' : 'is-theirs', { 'is-pending': message.status === 'pending' }]"
  >
    <div class="ft-bubble" :class="{ 'is-file': file }">
      <div v-if="file" class="ft-file">
        <span class="ft-file__icon"><ion-icon :icon="documentOutline" aria-hidden="true" /></span>
        <span class="ft-file__body">
          <span class="ft-file__name">{{ file.name }}</span>
          <span class="ft-file__meta">{{ file.size }}{{ fileState }}</span>
          <span
            v-if="file.state !== 'done'"
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
      </div>
      <p v-else class="ft-bubble__text">{{ message.text }}</p>

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
