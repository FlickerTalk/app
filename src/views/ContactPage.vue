<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonList,
  IonPage,
  IonSelect,
  IonSelectOption,
  IonToggle,
  IonToolbar,
} from "@ionic/vue";
import {
  banOutline,
  callOutline,
  chatbubbleOutline,
  checkmarkDoneOutline,
  flagOutline,
  hourglassOutline,
  notificationsOffOutline,
  pencilOutline,
  timerOutline,
} from "ionicons/icons";
import { useRoute } from "vue-router";
import Avatar from "../components/Avatar.vue";
import {
  block,
  chat,
  contactDetails,
  renameContact,
  reportContact,
  setHistory,
  setRules,
  type ContactDetails,
  type ContactRules,
} from "../core";
import { t } from "../i18n";

const route = useRoute();
const id = String(route.params.id);
const contact = computed(() => chat(id) ?? { name: "", hue: 0, connected: false });
const details = ref<ContactDetails | null>(null);

// Plan §29: every contact has a fingerprint both sides can compare in person, computed by the
// core from the two identity keys.
const fingerprint = computed(() => details.value?.fingerprint ?? "");
const blocked = computed(() => details.value?.blocked ?? false);

const name = ref("");
// Issue app#1: how long this phone keeps the conversation, and how long a read message stays.
// Seconds; 0 is forever and never. All of it is a choice of this phone.
const HISTORIES = [0, 30 * 86_400, 7 * 86_400, 86_400] as const;
const BURNS = [0, 60, 300, 3_600] as const;
const keepFor = ref(0);
const burnAfterRead = ref(0);

// Issues app#4–#6: what this phone takes from them and tells them. Nothing of it travels.
const rules = ref<ContactRules>({ muted: false, acceptsChat: true, acceptsCalls: true, receipts: true });
const RULES = [
  { key: "muted", label: "contact.mute", icon: notificationsOffOutline, test: "mute" },
  { key: "acceptsChat", label: "contact.acceptsChat", icon: chatbubbleOutline, test: "chat" },
  { key: "acceptsCalls", label: "contact.acceptsCalls", icon: callOutline, test: "calls" },
  { key: "receipts", label: "contact.receipts", icon: checkmarkDoneOutline, test: "receipts" },
] as const;

async function changeRule(key: keyof ContactRules, on: boolean) {
  rules.value = { ...rules.value, [key]: on };
  await setRules(id, rules.value);
}

onMounted(async () => {
  details.value = await contactDetails(id);
  name.value = details.value?.name ?? "";
  keepFor.value = details.value?.keepFor ?? 0;
  burnAfterRead.value = details.value?.burnAfterRead ?? 0;
  if (details.value?.rules) rules.value = { ...details.value.rules };
});

async function saveName() {
  if (name.value.trim()) await renameContact(id, name.value);
}

async function chooseHistory(seconds: number) {
  keepFor.value = seconds;
  await setHistory(id, keepFor.value, burnAfterRead.value);
}

async function chooseBurn(seconds: number) {
  burnAfterRead.value = seconds;
  await setHistory(id, keepFor.value, burnAfterRead.value);
}

const historyLabel = (seconds: number) =>
  seconds === 0 ? t("contact.forever") : t("contact.days", { days: Math.round(seconds / 86_400) });
const burnLabel = (seconds: number) =>
  seconds === 0 ? t("contact.never") : seconds >= 3_600 ? t("contact.hours", { hours: seconds / 3_600 }) : t("contact.minutes", { minutes: seconds / 60 });

// Plan §36: a report goes by email with a reason and, only if the user wants, the contact's last
// messages as evidence; the contact is blocked too.
const REASONS = ["spam", "abuse", "other"] as const;
const reporting = ref(false);
const reason = ref<(typeof REASONS)[number] | "">("");
const withEvidence = ref(false);

async function sendReport() {
  if (!reason.value) return;
  const evidence = withEvidence.value
    ? (chat(id)?.messages ?? []).filter((message) => !message.mine && message.text).slice(-5).map((message) => message.text)
    : [];
  await reportContact(id, reason.value, evidence);
  reporting.value = false;
  if (details.value) details.value.blocked = true;
}

// Plan §35: a blocked contact's messages, signals and mail are dropped on this phone.
async function toggleBlock() {
  const next = !blocked.value;
  await block(id, next);
  if (details.value) details.value.blocked = next;
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/chats" :aria-label="$t('common.back')" />
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-contact">
        <Avatar
          :name="contact.name"
          :hue="contact.hue"
          :size="88"
          :connected="contact.connected"
        />
        <h1 class="ft-contact__name">{{ contact.name }}</h1>
        <span class="ft-contact__status" :class="{ 'is-direct': contact.connected }">
          {{ contact.connected ? $t("chat.direct") : $t("chat.notConnected") }}
        </span>

        <section class="ft-contact__card">
          <span class="ft-contact__label">{{ $t("contact.fingerprint") }}</span>
          <code class="ft-contact__fingerprint" data-test="fingerprint">{{ fingerprint }}</code>
          <span class="ft-contact__hint">{{ $t("contact.verifyHint") }}</span>
        </section>

        <ion-list inset class="ft-group">
          <!-- Issue app#1: the name this phone shows, and the two history rules. -->
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="pencilOutline" aria-hidden="true" /></span>
            <input
              v-model="name"
              class="ft-contact__input"
              data-test="name"
              :aria-label="$t('contact.name')"
              :placeholder="$t('contact.name')"
              @keyup.enter="saveName"
            />
            <button
              slot="end"
              type="button"
              class="ft-contact__save"
              data-test="save-name"
              :disabled="!name.trim() || name.trim() === details?.name"
              @click="saveName"
            >
              {{ $t("common.save") }}
            </button>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="hourglassOutline" aria-hidden="true" /></span>
            <ion-select
              :value="keepFor"
              data-test="history"
              interface="popover"
              :label="$t('contact.history')"
              @ion-change="chooseHistory($event.detail.value)"
            >
              <ion-select-option v-for="option in HISTORIES" :key="option" :value="option">
                {{ historyLabel(option) }}
              </ion-select-option>
            </ion-select>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="timerOutline" aria-hidden="true" /></span>
            <ion-select
              :value="burnAfterRead"
              data-test="burn"
              interface="popover"
              :label="$t('contact.burn')"
              @ion-change="chooseBurn($event.detail.value)"
            >
              <ion-select-option v-for="option in BURNS" :key="option" :value="option">
                {{ burnLabel(option) }}
              </ion-select-option>
            </ion-select>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item v-for="rule in RULES" :key="rule.key" lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="rule.icon" aria-hidden="true" /></span>
            <ion-toggle
              :checked="rules[rule.key]"
              :data-test="rule.test"
              :aria-label="$t(rule.label)"
              @ion-change="changeRule(rule.key, $event.detail.checked)"
            >
              {{ $t(rule.label) }}
            </ion-toggle>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item button detail lines="none" data-test="block" @click="toggleBlock">
            <span slot="start" class="ft-tile"><ion-icon :icon="banOutline" aria-hidden="true" /></span>
            <ion-label>{{ blocked ? $t("contact.unblock") : $t("contact.block") }}</ion-label>
          </ion-item>
          <ion-item button detail lines="none" data-test="report" @click="reporting = !reporting">
            <span slot="start" class="ft-tile ft-tile--danger">
              <ion-icon :icon="flagOutline" aria-hidden="true" />
            </span>
            <ion-label color="danger">{{ $t("contact.report") }}</ion-label>
          </ion-item>
        </ion-list>

        <section v-if="reporting" class="ft-report" data-test="report-form">
          <span class="ft-contact__label">{{ $t("contact.reportWhy") }}</span>
          <div class="ft-report__reasons" role="radiogroup" :aria-label="$t('contact.reportWhy')">
            <button
              v-for="option in REASONS"
              :key="option"
              type="button"
              role="radio"
              class="ft-report__reason"
              :class="{ 'is-active': reason === option }"
              :aria-checked="reason === option"
              :data-test="`reason-${option}`"
              @click="reason = option"
            >
              {{ $t(`contact.reasons.${option}`) }}
            </button>
          </div>
          <label class="ft-report__evidence">
            <input v-model="withEvidence" type="checkbox" data-test="evidence" />
            {{ $t("contact.reportEvidence") }}
          </label>
          <button type="button" class="ft-report__send" data-test="send-report" :disabled="!reason" @click="sendReport">
            {{ $t("contact.reportSend") }}
          </button>
          <span class="ft-contact__hint">{{ $t("contact.reportHint") }}</span>
        </section>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-contact {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  max-width: 560px;
  margin: 0 auto;
  padding: var(--ft-space-2) 0 var(--ft-space-5);
  text-align: center;
}
.ft-contact__name {
  margin: var(--ft-space-3) 0 0;
  font-size: 24px;
  font-weight: 700;
}
.ft-contact__status {
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-contact__status.is-direct {
  color: var(--ft-accent);
}

.ft-contact__card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: calc(100% - var(--ft-space-5));
  margin: var(--ft-space-4) 0 var(--ft-space-2);
  padding: var(--ft-space-4);
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
}
.ft-contact__label,
.ft-contact__hint {
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-contact__fingerprint {
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
  line-height: 1.7;
  letter-spacing: 0.04em;
  overflow-wrap: anywhere;
}

.ft-group {
  width: 100%;
  --ion-item-background: var(--ft-surface);
  border: 1px solid var(--ft-border);
  text-align: start;
}
.ft-tile--danger {
  background: color-mix(in srgb, var(--ion-color-danger) 16%, transparent);
  color: var(--ion-color-danger);
}

.ft-report {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 100%;
  max-width: 520px;
  padding: 16px;
  border: 1px solid var(--ft-border);
  border-radius: 20px;
  background: var(--ft-surface);
}
.ft-report__reasons {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.ft-report__reason {
  padding: 8px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 999px;
  color: var(--ft-text);
  background: transparent;
}
.ft-report__reason.is-active {
  border-color: var(--ion-color-danger);
  color: #fff;
  background: var(--ion-color-danger);
}
.ft-report__evidence {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}
.ft-report__send {
  padding: 12px;
  border: 0;
  border-radius: 14px;
  font-weight: 600;
  color: #fff;
  background: var(--ion-color-danger);
}
.ft-report__send:disabled {
  opacity: 0.5;
}
</style>
