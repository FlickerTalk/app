<script setup lang="ts">
import { onMounted, ref } from "vue";
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
import { checkmarkOutline, scanOutline, shareOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import QrCode from "../components/QrCode.vue";
import { addContact, myCardLink, refreshChats, store } from "../core";
import { scanQr } from "../scanner";
import { t } from "../i18n";

// Plan §32: pairing happens through a signed Contact Card shared by QR, link or share sheet.
const router = useRouter();
const link = ref("");
const mode = ref<"code" | "scan">("code");
const copied = ref(false);
const pasted = ref("");
const error = ref("");

onMounted(async () => {
  link.value = await myCardLink();
});

async function share() {
  await navigator.clipboard?.writeText(link.value);
  copied.value = true;
}

// The camera reads the other phone's QR code: its content is the Contact Card link.
async function scanCode() {
  const read = await scanQr();
  if (read) await add(read);
}

async function add(value: string) {
  error.value = "";
  try {
    const id = await addContact(value);
    await refreshChats();
    router.replace(`/chat/${id}`);
  } catch {
    error.value = t("addContact.invalid");
  }
}
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
          <QrCode v-if="link" :value="link" :size="240" />
          <code class="ft-add__id">{{ store.me.id }}</code>
          <p class="ft-add__hint">{{ $t("addContact.myCodeHint") }}</p>
          <ion-button fill="outline" shape="round" :aria-label="$t('addContact.shareLink')" @click="share">
            <ion-icon slot="start" :icon="copied ? checkmarkOutline : shareOutline" aria-hidden="true" />
            {{ copied ? $t("addContact.copied") : $t("addContact.shareLink") }}
          </ion-button>
        </template>

        <template v-else>
          <button type="button" class="ft-add__scanner" data-test="scanner" @click="scanCode">
            <ion-icon :icon="scanOutline" aria-hidden="true" />
          </button>
          <ion-button shape="round" data-test="scan-now" @click="scanCode">
            <ion-icon slot="start" :icon="scanOutline" aria-hidden="true" />
            {{ $t("addContact.scanNow") }}
          </ion-button>
          <p class="ft-add__hint">{{ $t("addContact.scanHint") }}</p>

          <div class="ft-add__paste">
            <input
              v-model="pasted"
              data-test="paste"
              :placeholder="$t('addContact.paste')"
              :aria-label="$t('addContact.paste')"
              autocapitalize="off"
              spellcheck="false"
            />
            <button type="button" data-test="add" :disabled="!pasted.trim()" @click="add(pasted)">
              {{ $t("addContact.add") }}
            </button>
          </div>
          <p v-if="error" class="ft-add__error" role="alert">{{ error }}</p>
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

.ft-add__paste {
  display: flex;
  gap: 8px;
  width: min(100%, 360px);
}
.ft-add__paste input {
  flex: 1;
  min-width: 0;
  padding: 12px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 12px;
  background: var(--ft-surface);
  color: var(--ft-text);
  font: inherit;
}
.ft-add__paste button {
  appearance: none;
  padding: 0 18px;
  border: 0;
  border-radius: 12px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-add__paste button:disabled {
  opacity: 0.4;
  cursor: default;
}
.ft-add__error {
  margin: 0;
  color: var(--ion-color-danger);
  font-size: 14px;
}

.ft-add__scanner {
  appearance: none;
  cursor: pointer;
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
