<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  IonAlert,
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
import { useRoute, useRouter } from "vue-router";
import QrCode from "../components/QrCode.vue";
import { addContact, block, chat, myCardLink, refreshChats, shareText, store } from "../core";
import { scanQr } from "../scanner";
import { t } from "../i18n";
import { takeOpened, waitingFor } from "../opened";

// Plan §32: pairing happens through a signed Contact Card shared by QR, link or share sheet.
const router = useRouter();
const route = useRoute();
// Opened from a hidden session's QR button: the contact belongs to that session (Plan, 2026-09-23).
// Followed, not read once: a link that opens the app comes here without one, to the main list.
const session = computed(() => (typeof route.query.session === "string" ? route.query.session : undefined));
const link = ref("");
const mode = ref<"code" | "scan">("code");
const copied = ref(false);
const pasted = ref("");
const error = ref("");

watch(
  session,
  async (now) => {
    link.value = await myCardLink(now);
  },
  { immediate: true },
);

// A link that opened the app (2026-10-08): it goes in the field, ready to add. Nothing is sent
// until the user taps "Add": adding opens a session and sends our card to whoever made the link.
watch(
  () => waitingFor("add"),
  (waiting) => {
    const opened = waiting ? takeOpened("add") : null;
    if (!opened) return;
    mode.value = "scan";
    pasted.value = opened.link;
    error.value = opened.valid ? "" : t("addContact.invalid");
  },
  { immediate: true },
);

// The share sheet (WhatsApp, Signal, mail…) sends the link; only where there is none is it copied.
async function share() {
  try {
    await shareText(t("addContact.shareText", { link: link.value }));
  } catch {
    await navigator.clipboard?.writeText(link.value);
    copied.value = true;
  }
}

// The camera reads the other phone's QR code: its content is the Contact Card link.
async function scanCode() {
  const read = await scanQr();
  if (read) await add(read);
}

async function add(value: string) {
  error.value = "";
  try {
    const id = await addContact(value, session.value);
    await refreshChats();
    // app#78: adding someone blocked again leaves them blocked; a chat with them could not send.
    if (chat(id)?.blocked) blockedId.value = id;
    else router.replace(`/chat/${id}`);
  } catch {
    error.value = t("addContact.invalid");
  }
}

/** The contact just added who turned out to be blocked: asked about in an alert. */
const blockedId = ref<string | null>(null);
const blockedButtons = computed(() => [
  { text: t("common.cancel"), role: "cancel" },
  { text: t("contact.unblock"), handler: () => void unblock(blockedId.value) },
]);

async function unblock(id: string | null) {
  if (!id) return;
  await block(id, false);
  router.replace(`/chat/${id}`);
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/chats" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ $t("addContact.title") }}</span></ion-title>
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
          <button
            type="button"
            class="ft-add__scanner"
            data-test="scanner"
            :aria-label="$t('addContact.scanNow')"
            @click="scanCode"
          >
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
          <ion-alert
            :is-open="blockedId !== null"
            :header="$t('blocked.notice')"
            :message="$t('blocked.hint')"
            :buttons="blockedButtons"
            @did-dismiss="blockedId = null"
          />
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

/* A whole ID is 47 characters with no spaces: on a narrow phone it breaks anywhere, never runs
   off the screen (App Store screenshots, 2026-09-29). */
.ft-add__id {
  max-width: 100%;
  /* `break-all` for the WebKit of iOS 15.0-15.3, which has no `overflow-wrap: anywhere`. */
  word-break: break-all;
  overflow-wrap: anywhere;
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
