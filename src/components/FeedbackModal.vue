<script setup lang="ts">
import { computed, ref } from "vue";
import { IonIcon, IonTextarea } from "@ionic/vue";
import { closeOutline, sendOutline } from "ionicons/icons";
import { sendFeedback, type FeedbackOutcome } from "../core";

// 2026-10-02: an anonymous suggestion, mailed on by the router, which keeps nothing. Nothing of it
// is kept here either: no draft, no history; closing forgets it. A modal over Settings (Ioan), a
// sheet like the app's others: the ✕ or Android's Back (the page's) always close it; a tap outside
// closes it only with nothing written and nothing on its way, so a stray tap never loses a text.
// The router takes up to 2000 characters.
const MAX = 2000;

const emit = defineEmits<{ close: [] }>();

const text = ref("");
const sending = ref(false);
const outcome = ref<FeedbackOutcome | null>(null);
const ready = computed(() => !sending.value && text.value.trim() !== "");

function tappedOutside() {
  if (!sending.value && !text.value.trim()) emit("close");
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
  <div class="ft-feedback" data-test="feedback-modal" role="dialog" :aria-label="$t('feedback.title')" @click.self="tappedOutside">
    <div class="ft-feedback__card">
      <div class="ft-feedback__bar">
        <h2 class="ft-feedback__title">{{ $t("feedback.title") }}</h2>
        <button type="button" class="ft-feedback__close" data-test="feedback-close" :aria-label="$t('common.close')" @click="emit('close')">
          <ion-icon :icon="closeOutline" aria-hidden="true" />
        </button>
      </div>
      <ion-textarea
        v-model="text"
        class="ft-feedback__box"
        :rows="6"
        :maxlength="MAX"
        :placeholder="$t('feedback.placeholder')"
        :aria-label="$t('feedback.placeholder')"
      />
      <!-- Digits and a slash: always left to right, also in Arabic. -->
      <span class="ft-feedback__counter" dir="ltr" data-test="counter">{{ text.length }} / {{ MAX }}</span>
      <p class="ft-feedback__hint" data-test="hint">{{ $t("feedback.hint") }}</p>
      <button type="button" class="ft-feedback__send" data-test="send" :disabled="!ready" :aria-label="$t('feedback.send')" @click="send">
        <ion-icon :icon="sendOutline" aria-hidden="true" />
        {{ $t("feedback.send") }}
      </button>
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
  </div>
</template>

<style scoped>
/* A sheet over the page, as the permissions sheet and the contact picker: it sits on the keyboard
   (the page follows what can be seen, `viewport.ts`) and scrolls inside when there is no room. */
.ft-feedback {
  position: fixed;
  inset: 0;
  z-index: 30;
  display: grid;
  place-items: end center;
  padding: var(--ft-space-4);
  /* Above Android's navigation bar when the app runs edge to edge, as the composer keeps itself. */
  padding-bottom: calc(var(--ft-space-4) + var(--ion-safe-area-bottom, 0px));
  background: rgba(0, 0, 0, 0.35);
}
.ft-feedback__card {
  display: flex;
  flex-direction: column;
  gap: var(--ft-space-3);
  width: min(100%, 520px);
  max-height: 100%;
  overflow-y: auto;
  padding: var(--ft-space-3) var(--ft-space-4) var(--ft-space-4);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
  color: var(--ft-text);
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.3);
}
.ft-feedback__bar {
  display: flex;
  align-items: center;
  gap: var(--ft-space-2);
}
.ft-feedback__title {
  flex: 1;
  margin: 0;
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-feedback__close {
  display: grid;
  place-items: center;
  width: 44px;
  height: 44px;
  flex: none;
  border: 0;
  border-radius: 50%;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font-size: 20px;
  cursor: pointer;
}
.ft-feedback__box {
  --padding-start: 12px;
  --padding-end: 12px;
  border: 1px solid var(--ft-border);
  border-radius: 12px;
  background: var(--ft-bg);
}
.ft-feedback__counter {
  align-self: flex-end;
  margin-top: calc(-1 * var(--ft-space-2));
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-feedback__hint {
  margin: 0;
  font-size: 14px;
  line-height: 1.4;
  color: var(--ft-muted);
  text-align: start;
}
.ft-feedback__send {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: var(--ft-space-2);
  min-height: 44px;
  padding: 0 18px;
  border: 0;
  border-radius: 999px;
  color: var(--ft-on-accent);
  background: var(--ft-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-feedback__send:disabled {
  opacity: 0.45;
  cursor: default;
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
