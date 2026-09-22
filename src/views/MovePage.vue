<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonPage,
  IonTitle,
  IonToolbar,
} from "@ionic/vue";
import { checkmarkCircleOutline, scanOutline, swapHorizontalOutline, warningOutline } from "ionicons/icons";
import { useRoute } from "vue-router";
import QrCode from "../components/QrCode.vue";
import { invite, move, moveTo } from "../moving";
import { scanQr } from "../scanner";

// Plan §60: the new phone shows a QR (its card and a one-time secret, never a key); the old
// phone scans it and hands over the identity, contacts and history, directly. The old phone is
// then erased: one identity, one phone.
const route = useRoute();
const isNew = computed(() => route.query.role === "new");
const link = ref("");
const pasted = ref("");
const percent = computed(() => (move.total ? Math.round((move.done / move.total) * 100) : 0));

onMounted(async () => {
  if (isNew.value) link.value = await invite();
});

async function scanInvite() {
  // Cancelled or no camera: the link can be pasted instead.
  const read = await scanQr();
  if (read) await moveTo(read);
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button :default-href="isNew ? '/welcome' : '/tabs/settings'" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ isNew ? $t("move.fromOld") : $t("move.toNew") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ion-padding">
      <div class="ft-move">
        <template v-if="move.phase === 'received' || move.phase === 'sent'">
          <ion-icon :icon="checkmarkCircleOutline" class="ft-move__big is-done" aria-hidden="true" />
          <p class="ft-move__title">{{ move.phase === "received" ? $t("move.received") : $t("move.sent") }}</p>
          <p class="ft-move__hint">{{ $t("move.restarting") }}</p>
        </template>

        <template v-else-if="move.phase === 'moving'">
          <ion-icon :icon="swapHorizontalOutline" class="ft-move__big" aria-hidden="true" />
          <p class="ft-move__title">{{ $t("move.moving") }}</p>
          <span
            class="ft-move__progress"
            role="progressbar"
            aria-valuemin="0"
            aria-valuemax="100"
            :aria-valuenow="percent"
            :aria-label="$t('move.moving')"
          >
            <span class="ft-move__bar" :style="{ width: `${percent}%` }" />
          </span>
          <p class="ft-move__hint">{{ $t("move.keepClose") }}</p>
        </template>

        <template v-else-if="isNew">
          <QrCode v-if="link" :value="link" :size="240" />
          <p class="ft-move__hint">{{ $t("move.scanThis") }}</p>
          <p class="ft-move__hint">{{ $t("move.noKey") }}</p>
        </template>

        <template v-else>
          <ion-icon :icon="warningOutline" class="ft-move__big is-warning" aria-hidden="true" />
          <p class="ft-move__title">{{ $t("move.everything") }}</p>
          <p class="ft-move__hint">{{ $t("move.erased") }}</p>
          <ion-button shape="round" data-test="move-scan" @click="scanInvite">
            <ion-icon slot="start" :icon="scanOutline" aria-hidden="true" />
            {{ $t("move.scan") }}
          </ion-button>
          <div class="ft-move__paste">
            <input
              v-model="pasted"
              type="text"
              class="ft-move__input"
              :placeholder="$t('move.paste')"
              :aria-label="$t('move.paste')"
              data-test="move-link"
            />
            <button type="button" class="ft-move__go" data-test="move-go" :disabled="!pasted.trim()" @click="moveTo(pasted)">
              {{ $t("move.go") }}
            </button>
          </div>
        </template>

        <p v-if="move.phase === 'failed'" class="ft-move__error" role="alert">
          {{ $t("move.failed") }}<template v-if="move.error">: {{ move.error }}</template>
        </p>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-move {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-4);
  max-width: 460px;
  margin: 6vh auto 0;
  text-align: center;
}
.ft-move__big {
  font-size: 64px;
  color: var(--ft-accent);
}
.ft-move__big.is-done {
  color: var(--ion-color-success);
}
.ft-move__big.is-warning {
  color: var(--ion-color-warning);
}
.ft-move__title {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
}
.ft-move__hint {
  margin: 0;
  color: var(--ft-muted);
  font-size: 14px;
}
.ft-move__progress {
  display: block;
  width: 100%;
  height: 6px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--ft-surface-2);
}
.ft-move__bar {
  display: block;
  height: 100%;
  background: var(--ft-accent);
  transition: width 0.3s ease;
}
.ft-move__paste {
  display: flex;
  gap: 8px;
  width: 100%;
}
.ft-move__input {
  flex: 1;
  min-width: 0;
  padding: 12px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 14px;
  color: var(--ft-text);
  background: var(--ft-surface);
}
.ft-move__go {
  padding: 0 16px;
  border: 0;
  border-radius: 14px;
  font-weight: 600;
  color: #fff;
  background: var(--ion-color-danger);
}
.ft-move__go:disabled {
  opacity: 0.5;
}
.ft-move__error {
  color: var(--ion-color-danger);
}
</style>
