<script setup lang="ts">
import { computed, ref } from "vue";
import {
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonModal,
  IonText,
  IonTextarea,
  IonTitle,
  IonToolbar,
} from "@ionic/vue";
import { closeOutline, sendOutline } from "ionicons/icons";
import { sendFeedback, type FeedbackOutcome } from "../core";

// no draft is kept, the text is gone with the modal. The router takes up to 2000 characters.
const MAX = 2000;

defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const text = ref("");
const sending = ref(false);
const outcome = ref<FeedbackOutcome | null>(null);
const ready = computed(() => !sending.value && text.value.trim() !== "");

const INCIDENTAL = new Set(["backdrop", "gesture"]);

async function canDismiss(_data?: unknown, role?: string): Promise<boolean> {
  return !INCIDENTAL.has(role ?? "") || (!sending.value && text.value.trim() === "");
}

// v-model fires only on typing, not when the box is emptied after a send
function edited(value: string | null | undefined) {
  text.value = value ?? "";
  outcome.value = null;
}

function dismissed() {
  text.value = "";
  outcome.value = null;
  emit("close");
}

async function send() {
  const written = text.value.trim();
  if (sending.value || !written) return;
  sending.value = true;
  outcome.value = null;
  try {
    outcome.value = await sendFeedback(written);
    if (outcome.value === "sent") text.value = "";
  } finally {
    sending.value = false;
  }
}
</script>

<template>
  <ion-modal :is-open="open" :can-dismiss="canDismiss" data-test="feedback-modal" @did-dismiss="dismissed">
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-title><span class="ft-title">{{ $t("feedback.title") }}</span></ion-title>
        <ion-buttons slot="end">
          <ion-button fill="clear" data-test="feedback-close" :aria-label="$t('common.close')" @click="emit('close')">
            <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ion-padding">
      <ion-textarea
        :model-value="text"
        class="ft-feedback__box"
        fill="outline"
        :rows="6"
        :counter="true"
        :maxlength="MAX"
        :placeholder="$t('feedback.placeholder')"
        :aria-label="$t('feedback.placeholder')"
        @update:model-value="edited"
      />
      <ion-text color="medium">
        <p data-test="hint">{{ $t("feedback.hint") }}</p>
      </ion-text>
      <ion-button expand="block" shape="round" data-test="send" :disabled="!ready" @click="send">
        <ion-icon slot="start" :icon="sendOutline" aria-hidden="true" />
        {{ $t("feedback.send") }}
      </ion-button>
      <ion-text
        v-if="outcome"
        class="ft-feedback__outcome"
        :color="outcome === 'sent' ? 'success' : 'danger'"
        :role="outcome === 'sent' ? 'status' : 'alert'"
        data-test="outcome"
      >
        <p>{{ $t(`feedback.${outcome}`) }}</p>
      </ion-text>
    </ion-content>
  </ion-modal>
</template>

<style scoped>
/* the counter has no letters to take a direction from, so it read LTR in Arabic */
.ft-feedback__box :deep(.counter) {
  unicode-bidi: plaintext;
}
/* Ionic's ios mode has no outline (fill is md only) */
.ft-feedback__box.ios {
  --padding-start: 12px;
  --padding-end: 12px;
  --border-width: 0;
  padding-bottom: 6px;
  border: 1px solid var(--ion-color-medium);
  border-radius: 12px;
}
.ft-feedback__outcome {
  display: block;
  text-align: center;
}
</style>
