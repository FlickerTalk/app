<script setup lang="ts">
import { computed, ref } from "vue";
import { IonBackButton, IonButton, IonButtons, IonContent, IonHeader, IonIcon, IonPage, IonTextarea, IonTitle, IonToolbar } from "@ionic/vue";
import { sendOutline } from "ionicons/icons";
import { sendFeedback, type FeedbackOutcome } from "../core";

// 2026-10-02: an anonymous suggestion, mailed on by the router, which keeps nothing. Nothing of it
// is kept here either: no draft, no history. The router takes up to 2000 characters.
const MAX = 2000;

const text = ref("");
const sending = ref(false);
const outcome = ref<FeedbackOutcome | null>(null);
const ready = computed(() => !sending.value && text.value.trim() !== "");

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
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("feedback.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ion-padding">
      <div class="ft-feedback">
        <ion-textarea
          v-model="text"
          class="ft-feedback__box"
          :rows="6"
          :auto-grow="true"
          :maxlength="MAX"
          :placeholder="$t('feedback.placeholder')"
          :aria-label="$t('feedback.placeholder')"
        />
        <!-- Digits and a slash: always left to right, also in Arabic. -->
        <span class="ft-feedback__counter" dir="ltr" data-test="counter">{{ text.length }} / {{ MAX }}</span>
        <p class="ft-feedback__hint" data-test="hint">{{ $t("feedback.hint") }}</p>
        <ion-button shape="round" data-test="send" :disabled="!ready" :aria-label="$t('feedback.send')" @click="send">
          <ion-icon slot="start" :icon="sendOutline" aria-hidden="true" />
          {{ $t("feedback.send") }}
        </ion-button>
        <p
          v-if="outcome"
          class="ft-feedback__outcome"
          :class="{ 'is-sent': outcome === 'sent' }"
          :role="outcome === 'sent' ? 'status' : 'alert'"
          data-test="outcome"
        >
          {{ $t(`feedback.${outcome}`) }}
        </p>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-feedback {
  display: flex;
  flex-direction: column;
  gap: var(--ft-space-3);
  max-width: 560px;
  margin: 0 auto;
}
.ft-feedback__box {
  --padding-start: 12px;
  --padding-end: 12px;
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
}
.ft-feedback__counter {
  align-self: flex-end;
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-feedback__hint {
  margin: 0;
  font-size: 14px;
  color: var(--ft-muted);
  text-align: start;
}
.ft-feedback__outcome {
  margin: 0;
  text-align: center;
  color: var(--ion-color-danger);
}
.ft-feedback__outcome.is-sent {
  color: var(--ft-accent);
}
</style>
