<script setup lang="ts">
import { IonButton, IonIcon } from "@ionic/vue";
import {
  arrowRedoOutline,
  arrowUndoOutline,
  contractOutline,
  expandOutline,
  extensionPuzzleOutline,
  lockClosedOutline,
  openOutline,
  pencilOutline,
  pin,
  pinOutline,
  shareOutline,
  trashOutline,
} from "ionicons/icons";
import { isLocked, pluginName } from "../plugins";
import type { Chat, ChatMessage, PluginView } from "../core";

defineProps<{
  message?: ChatMessage;
  folded: boolean;
  canEdit: boolean;
  request: boolean;
  forwarding: boolean;
  openingWith: boolean;
  erasing: boolean;
  others: Chat[];
  openers: PluginView[];
  viewable: boolean;
}>();
const emit = defineEmits<{
  "update:forwarding": [on: boolean];
  "update:openingWith": [on: boolean];
  "update:erasing": [on: boolean];
  close: [];
  react: [emoji: string];
  edit: [];
  pin: [];
  reply: [];
  fold: [];
  share: [];
  erase: [];
  eraseEverywhere: [];
  forwardTo: [contact: string];
  openWith: [plugin: string];
  openElsewhere: [];
}>();

const QUICK_REACTIONS = ["👍", "❤️", "😂", "😮", "😢", "🙏"];
</script>

<template>
  <div class="ft-actions" data-test="actions" @click.self="emit('close')">
    <div v-if="forwarding" class="ft-actions__bar">
      <span class="ft-actions__title">{{ $t("chat.forwardTo") }}</span>
      <button
        v-for="one in others"
        :key="one.id"
        type="button"
        class="ft-actions__to"
        :data-test="`to-${one.id}`"
        dir="auto"
        @click="emit('forwardTo', one.id)"
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
        @click="emit('openWith', one.id)"
      >
        <ion-icon v-if="isLocked(one)" :icon="lockClosedOutline" :aria-label="$t('plugins.locked')" />
        {{ pluginName(one) }}
      </button>
      <button v-if="viewable" type="button" class="ft-actions__to" data-test="open-elsewhere" @click="emit('openElsewhere')">
        <ion-icon :icon="openOutline" aria-hidden="true" /> {{ $t("chat.otherApp") }}
      </button>
    </div>
    <div v-else class="ft-actions__bar">
      <span v-if="!request" class="ft-actions__emoji" role="group" :aria-label="$t('chat.react')">
        <button
          v-for="emoji in QUICK_REACTIONS"
          :key="emoji"
          type="button"
          class="ft-actions__react"
          :class="{ 'is-active': message?.reactions?.mine === emoji }"
          :data-test="`react-${emoji}`"
          :aria-label="message?.reactions?.mine === emoji ? $t('chat.unreact') : $t('chat.reactWith', { emoji })"
          :aria-pressed="message?.reactions?.mine === emoji"
          @click="emit('react', emoji)"
        >
          {{ emoji }}
        </button>
      </span>
      <ion-button v-if="canEdit" fill="clear" color="medium" data-test="edit" :aria-label="$t('chat.edit')" @click="emit('edit')">
        <ion-icon slot="icon-only" :icon="pencilOutline" aria-hidden="true" />
      </ion-button>
      <ion-button
        fill="clear"
        :color="message?.pinned ? 'primary' : 'medium'"
        data-test="pin"
        :aria-label="$t(message?.pinned ? 'chat.unpin' : 'chat.pin')"
        @click="emit('pin')"
      >
        <ion-icon slot="icon-only" :icon="message?.pinned ? pin : pinOutline" aria-hidden="true" />
      </ion-button>
      <ion-button v-if="!request" fill="clear" color="medium" data-test="reply" :aria-label="$t('chat.reply')" @click="emit('reply')">
        <ion-icon slot="icon-only" :icon="arrowUndoOutline" aria-hidden="true" />
      </ion-button>
      <button type="button" class="ft-round ft-round--ghost" data-test="fold" :aria-label="$t(folded ? 'chat.unfold' : 'chat.fold')" @click="emit('fold')">
        <ion-icon :icon="folded ? expandOutline : contractOutline" aria-hidden="true" />
      </button>
      <button type="button" class="ft-round ft-round--ghost" data-test="forward" :aria-label="$t('chat.forward')" @click="emit('update:forwarding', true)">
        <ion-icon :icon="arrowRedoOutline" aria-hidden="true" />
      </button>
      <button type="button" class="ft-round ft-round--ghost" data-test="share" :aria-label="$t('chat.share')" @click="emit('share')">
        <ion-icon :icon="shareOutline" aria-hidden="true" />
      </button>
      <button
        v-if="openers.length"
        type="button"
        class="ft-round ft-round--ghost"
        data-test="open-with"
        :aria-label="$t('chat.openWithPlugin')"
        @click="emit('update:openingWith', true)"
      >
        <ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" />
      </button>
      <button
        v-if="!erasing"
        type="button"
        class="ft-round ft-round--ghost ft-actions__danger"
        data-test="delete"
        :aria-label="$t('chat.delete')"
        @click="emit('update:erasing', true)"
      >
        <ion-icon :icon="trashOutline" aria-hidden="true" />
      </button>
      <template v-else>
        <button type="button" class="ft-actions__sure" data-test="delete-sure" @click="emit('erase')">
          {{ $t("chat.deleteSure") }}
        </button>
        <button
          v-if="message?.mine && !message.deleted && !message.scheduledFor"
          type="button"
          class="ft-actions__sure"
          data-test="delete-everyone"
          @click="emit('eraseEverywhere')"
        >
          {{ $t("chat.deleteEveryone") }}
        </button>
      </template>
    </div>
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
  padding-left: calc(var(--ft-space-4) + var(--ion-safe-area-left, 0px));
  padding-right: calc(var(--ft-space-4) + var(--ion-safe-area-right, 0px));
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
.ft-actions__emoji {
  flex-basis: 100%;
  display: flex;
  justify-content: space-around;
  gap: 2px;
}
.ft-actions__react {
  appearance: none;
  width: 44px;
  height: 44px;
  border: 0;
  border-radius: 50%;
  background: transparent;
  font-size: 24px;
  line-height: 1;
  cursor: pointer;
}
.ft-actions__react.is-active {
  background: rgba(var(--ion-color-primary-rgb), 0.22);
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
</style>
