<script setup lang="ts">
import { computed, ref } from "vue";
import { IonButton, IonFabButton, IonFabList, IonIcon, IonTextarea } from "@ionic/vue";
import { add, arrowUp, cameraOutline, closeOutline, documentOutline, happyOutline, imageOutline, micOutline, timeOutline, trashOutline } from "ionicons/icons";
import EmojiPicker from "./EmojiPicker.vue";
import type { ChatMessage, PickedFile } from "../core";

const props = defineProps<{
  draft: string;
  emoji: boolean;
  attaching: boolean;
  error: string;
  staged: PickedFile | null;
  editing: ChatMessage | null;
  replying: ChatMessage | null;
  contactName?: string;
  playing: boolean;
  recording: boolean;
  elapsed: string;
}>();
const emit = defineEmits<{
  "update:draft": [text: string];
  "update:emoji": [open: boolean];
  "update:attaching": [open: boolean];
  addEmoji: [emoji: string];
  send: [];
  sendLater: [];
  record: [];
  sendVoice: [];
  discardVoice: [];
  pick: [accept: string];
  snap: [];
  attach: [event: Event];
  cancelEdit: [];
  cancelReply: [];
  discardStaged: [];
  sendStaged: [];
}>();

const text = computed({
  get: () => props.draft,
  set: (value: string) => emit("update:draft", value),
});

const replyingText = computed(() => (props.replying?.kind === "file" ? props.replying.file?.name ?? "" : props.replying?.text ?? ""));

const picker = ref<HTMLInputElement | null>(null);
defineExpose({ picker });
</script>

<template>
  <p v-if="error" class="ft-composer__error" role="alert">{{ error }}</p>
  <div v-if="staged" class="ft-staged" data-test="staged">
    <ion-icon :icon="documentOutline" aria-hidden="true" />
    <span class="ft-staged__name">{{ staged.name }}</span>
    <button type="button" class="ft-round ft-round--ghost" data-test="staged-discard" :aria-label="$t('chat.discardAttachment')" @click="emit('discardStaged')">
      <ion-icon :icon="trashOutline" aria-hidden="true" />
    </button>
    <button type="button" class="ft-round ft-round--send" data-test="staged-send" :aria-label="$t('chat.send')" @click="emit('sendStaged')">
      <ion-icon :icon="arrowUp" aria-hidden="true" />
    </button>
  </div>
  <!-- the composer below is not an ion-toolbar, that one clips the «+» menu -->
  <div v-if="editing" class="ft-replying ft-replying--edit" data-test="editing">
    <span class="ft-replying__text">
      <span class="ft-replying__who">{{ $t("chat.editing") }}</span>
      <span class="ft-replying__quote" dir="auto">{{ editing.text }}</span>
    </span>
    <ion-button fill="clear" color="medium" data-test="cancel-edit" :aria-label="$t('chat.cancelEdit')" @click="emit('cancelEdit')">
      <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
    </ion-button>
  </div>
  <div v-if="replying" class="ft-replying" data-test="replying">
    <span class="ft-replying__text">
      <span class="ft-replying__who">{{ replying.mine ? $t("chat.you") : contactName }}</span>
      <span class="ft-replying__quote" dir="auto">{{ replyingText }}</span>
    </span>
    <ion-button fill="clear" color="medium" data-test="cancel-reply" :aria-label="$t('chat.cancelReply')" @click="emit('cancelReply')">
      <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
    </ion-button>
  </div>
  <div class="ft-composer">
    <div class="ft-composer__row">
      <div v-if="!playing" class="ft-attach">
        <button
          type="button"
          class="ft-round ft-round--ghost"
          :class="{ 'is-open': attaching }"
          :aria-label="$t('chat.attach')"
          :aria-expanded="attaching"
          @click="emit('update:attaching', !attaching)"
        >
          <ion-icon :icon="add" aria-hidden="true" />
        </button>
        <!-- no ion-fab around it, it would take the first option for its main button -->
        <ion-fab-list side="top" class="ft-attach__list" :activated="attaching">
          <ion-fab-button :aria-label="$t('chat.attachMedia')" @click="emit('pick', 'image/*,video/*')">
            <ion-icon :icon="imageOutline" aria-hidden="true" />
          </ion-fab-button>
          <ion-fab-button :aria-label="$t('chat.attachCamera')" @click="emit('snap')">
            <ion-icon :icon="cameraOutline" aria-hidden="true" />
          </ion-fab-button>
          <ion-fab-button :aria-label="$t('chat.attachFile')" @click="emit('pick', '')">
            <ion-icon :icon="documentOutline" aria-hidden="true" />
          </ion-fab-button>
        </ion-fab-list>
      </div>
      <input ref="picker" type="file" multiple hidden @change="emit('attach', $event)" />
      <button
        v-if="!recording"
        type="button"
        class="ft-round ft-round--ghost"
        :class="{ 'is-open': emoji }"
        :aria-label="$t('chat.emoji')"
        :aria-pressed="emoji"
        data-test="open-emoji"
        @click="emit('update:emoji', !emoji)"
      >
        <ion-icon :icon="happyOutline" aria-hidden="true" />
      </button>
      <div v-if="recording" class="ft-recording" data-test="recording">
        <span class="ft-recording__dot" aria-hidden="true" />
        <span class="ft-recording__time">{{ elapsed }}</span>
        <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('chat.discardVoice')" @click="emit('discardVoice')">
          <ion-icon :icon="trashOutline" aria-hidden="true" />
        </button>
      </div>
      <ion-textarea
        v-else
        v-model="text"
        class="ft-composer__input"
        :auto-grow="true"
        :rows="1"
        :placeholder="$t('chat.message')"
        :aria-label="$t('chat.message')"
      />
      <button
        v-if="recording"
        type="button"
        class="ft-round ft-round--send"
        :aria-label="$t('chat.sendVoice')"
        @click="emit('sendVoice')"
      >
        <ion-icon :icon="arrowUp" aria-hidden="true" />
      </button>
      <button
        v-if="!recording && draft.trim() && !editing"
        type="button"
        class="ft-round ft-round--ghost"
        data-test="send-later"
        :aria-label="$t('chat.sendLater')"
        @click="emit('sendLater')"
      >
        <ion-icon :icon="timeOutline" aria-hidden="true" />
      </button>
      <button v-if="!recording && draft.trim()" type="button" class="ft-round ft-round--send" :aria-label="$t('chat.send')" @click="emit('send')">
        <ion-icon :icon="arrowUp" aria-hidden="true" />
      </button>
      <button v-else-if="!recording" type="button" class="ft-round ft-round--send" :aria-label="$t('chat.record')" @click="emit('record')">
        <ion-icon :icon="micOutline" aria-hidden="true" />
      </button>
    </div>
  </div>
  <emoji-picker v-if="emoji" @pick="emit('addEmoji', $event)" />
</template>

<style scoped>
.ft-replying {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-block: 0 6px;
  margin-inline: var(--ft-space-4);
  padding-block: 2px;
  padding-inline: 12px 0;
  border-inline-start: 3px solid var(--ion-color-primary);
  border-radius: 10px;
  background: var(--ion-color-light-shade);
}
.ft-replying__text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.ft-replying__who {
  font-size: var(--ft-font-meta);
  font-weight: 600;
  color: var(--ion-color-primary);
}
.ft-replying__quote {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: var(--ft-font-meta);
  color: var(--ion-color-medium);
}

.ft-round--ghost.is-open {
  color: var(--ft-accent);
}

.ft-composer {
  padding: 6px 8px calc(8px + var(--ion-safe-area-bottom, 0px));
  background: var(--ft-bg);
}
/* sibling selector, iOS 15.0–15.3 has no :has() */
.ft-composer + .ft-emoji {
  margin-top: calc(-1 * var(--ion-safe-area-bottom, 0px));
  padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-composer__row {
  display: flex;
  align-items: flex-end;
  gap: 8px;
}
.ft-attach {
  position: relative;
  flex-shrink: 0;
}
.ft-attach__list {
  inset-inline-start: 0;
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
