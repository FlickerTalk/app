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

// 2026-10-02: an anonymous suggestion, mailed on by the router, which keeps nothing. Nothing of it
// is kept here either: no draft, no history; once the modal is gone, it is forgotten. An Ionic
// modal over Settings (Ioan): the ✕ and Android's Back (Settings sets `open` to false) always
// close it; a tap outside, Escape or a swipe only with nothing written and nothing on its way,
// so a stray tap never loses a text. The router takes up to 2000 characters.
const MAX = 2000;

defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const text = ref("");
const sending = ref(false);
const outcome = ref<FeedbackOutcome | null>(null);
const ready = computed(() => !sending.value && text.value.trim() !== "");

// Ionic's roles for what the user did by the way: `backdrop` (a tap outside, Escape) and `gesture`.
const INCIDENTAL = new Set(["backdrop", "gesture"]);

async function canDismiss(_data?: unknown, role?: string): Promise<boolean> {
  return !INCIDENTAL.has(role ?? "") || (!sending.value && text.value.trim() === "");
}

function dismissed() {
  text.value = "";
  outcome.value = null;
  emit("close");
}

// "sent" only when the router took it; otherwise the text stays, so nothing written is lost.
// While a send is on its way the button is off and another tap does nothing.
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
        <ion-title>{{ $t("feedback.title") }}</ion-title>
        <ion-buttons slot="end">
          <ion-button fill="clear" data-test="feedback-close" :aria-label="$t('common.close')" @click="emit('close')">
            <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ion-padding">
      <ion-textarea
        v-model="text"
        class="ft-feedback__box"
        fill="outline"
        :rows="6"
        :counter="true"
        :maxlength="MAX"
        :placeholder="$t('feedback.placeholder')"
        :aria-label="$t('feedback.placeholder')"
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
/* Ionic's counter ("15 / 2000") has no letters to take a direction from: on its own it reads left
   to right, also in Arabic, and still sits at the end of the line. */
.ft-feedback__box :deep(.counter) {
  unicode-bidi: plaintext;
}
.ft-feedback__outcome {
  display: block;
  text-align: center;
}
</style>
