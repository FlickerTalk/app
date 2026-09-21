<script setup lang="ts">
import { ref } from "vue";
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
import { scanOutline, shareOutline } from "ionicons/icons";
import QrCode from "../components/QrCode.vue";
import data from "../mock/chats.json";

// Plan §32: pairing happens through a signed Contact Card shared by QR, link or share sheet.
const link = `https://flickertalk.com/c/${data.me.id}`;
const mode = ref<"code" | "scan">("code");
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/chats" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("addContact.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-add">
        <div class="ft-add__modes" role="group" :aria-label="$t('addContact.title')">
          <button
            type="button"
            class="ft-add__mode"
            :class="{ 'is-active': mode === 'code' }"
            :aria-pressed="mode === 'code'"
            data-test="mode-code"
            @click="mode = 'code'"
          >
            {{ $t("addContact.myCode") }}
          </button>
          <button
            type="button"
            class="ft-add__mode"
            :class="{ 'is-active': mode === 'scan' }"
            :aria-pressed="mode === 'scan'"
            data-test="mode-scan"
            @click="mode = 'scan'"
          >
            {{ $t("addContact.scan") }}
          </button>
        </div>

        <template v-if="mode === 'code'">
          <QrCode :value="link" :size="240" />
          <code class="ft-add__id">{{ data.me.id }}</code>
          <p class="ft-add__hint">{{ $t("addContact.myCodeHint") }}</p>
          <ion-button fill="outline" shape="round" :aria-label="$t('addContact.shareLink')">
            <ion-icon slot="start" :icon="shareOutline" aria-hidden="true" />
            {{ $t("addContact.shareLink") }}
          </ion-button>
        </template>

        <template v-else>
          <div class="ft-add__scanner" data-test="scanner">
            <ion-icon :icon="scanOutline" aria-hidden="true" />
          </div>
          <p class="ft-add__hint">{{ $t("addContact.scanHint") }}</p>
        </template>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-add {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-4);
  padding: var(--ft-space-4) var(--ft-space-4) var(--ft-space-5);
  text-align: center;
}

.ft-add__modes {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 12px;
  background: var(--ft-surface-2);
}
.ft-add__mode {
  appearance: none;
  padding: 8px 18px;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--ft-muted);
  font: inherit;
  font-size: 14px;
  cursor: pointer;
}
.ft-add__mode.is-active {
  background: var(--ft-surface);
  color: var(--ft-text);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25);
}

.ft-add__id {
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
  color: var(--ft-muted);
}
.ft-add__hint {
  margin: 0;
  max-width: 28ch;
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.4;
}

.ft-add__scanner {
  display: grid;
  place-items: center;
  width: 264px;
  height: 264px;
  border: 2px dashed var(--ft-border);
  border-radius: 24px;
  background: var(--ft-surface);
  color: var(--ft-accent);
  font-size: 64px;
}
</style>
