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
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { useRoute, useRouter } from "vue-router";
import { closeOnBackWhile, goBack } from "../back";
import PluginSheet from "../components/PluginSheet.vue";
import PermissionAsk from "../components/PermissionAsk.vue";
import ContactPicker from "../components/ContactPicker.vue";
import { offerSend, type ContactAsk, type Proposal } from "../pending-send";
import { lockClosedOutline } from "ionicons/icons";
import { installed, isLocked, pluginIcon, pluginImage, pluginName, refreshPlugins, refreshPremiumLock } from "../plugins";
import type { PermissionNeed } from "../permissions";
import { PREMIUM_PAGE } from "../core";

const route = useRoute();
const router = useRouter();
const id = String(route.params.id);
// a reminder tapped while this page is open only changes the query
const reminder = computed(() => (typeof route.query.reminder === "string" ? route.query.reminder : undefined));
const session = computed(() => (typeof route.query.session === "string" && route.query.session ? route.query.session : undefined));
const plugin = computed(() => installed.value.find((one) => one.id === id));
const ready = ref(false);

onMounted(async () => {
  await Promise.all([plugin.value ? undefined : refreshPlugins(), refreshPremiumLock()]);
  ready.value = true;
});
const locked = computed(() => Boolean(plugin.value && isLocked(plugin.value)));

// leaving closes the plugin through its sheet so it can say goodbye while the page goes
const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);
const opening = ref(0);
const here = ref(false);
let saidGoodbye = false;
onIonViewWillEnter(() => {
  here.value = true;
  if (saidGoodbye) reopen();
});
onIonViewWillLeave(() => {
  here.value = false;
  answerNeed(false);
  answerPick(null);
  void sheet.value?.close();
});

const needing = ref<PermissionNeed | null>(null);
function answerNeed(allowed: boolean) {
  const need = needing.value;
  needing.value = null;
  need?.answer(allowed);
}
closeOnBackWhile(() => here.value && Boolean(needing.value), () => answerNeed(false));
const choosing = ref<ContactAsk | null>(null);
function answerPick(contact: string | null) {
  const ask = choosing.value;
  choosing.value = null;
  ask?.answer(contact);
}
closeOnBackWhile(() => here.value && Boolean(choosing.value), () => answerPick(null));
function addContact() {
  answerPick(null);
  void router.push("/add-contact");
}
function sendTo(contact: string, proposal: Proposal | null) {
  if (proposal) offerSend(contact, proposal);
  void router.replace(`/chat/${contact}`);
}

function closed() {
  if (here.value) reopen();
  else saidGoodbye = true;
}
function reopen() {
  saidGoodbye = false;
  opening.value += 1;
}

// Android's WebView skips history entries added without a touch (a reminder tapped in the
// notifications opens this page that way), so Back left to the system put the app away.
function leave(): Promise<unknown> {
  return window.history.state?.back ? goBack(router) : router.replace("/tabs/apps");
}
closeOnBackWhile(() => here.value, leave);
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/apps" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ plugin ? pluginName(plugin) : "" }}</span></ion-title>
      </ion-toolbar>
    </ion-header>
    <ion-content class="ft-plugin-page" :scroll-y="false">
      <div v-if="ready && plugin && locked" class="ft-plugin-page__missing" data-test="locked">
        <ion-icon :icon="lockClosedOutline" class="ft-plugin-page__lock" aria-hidden="true" />
        <p>{{ $t("plugins.lockedHint") }}</p>
        <ion-button data-test="subscribe" @click="router.push(PREMIUM_PAGE)">{{ $t("plugins.subscribe") }}</ion-button>
      </div>
      <PluginSheet
        v-else-if="ready && plugin"
        ref="sheet"
        :key="opening"
        :plugin="plugin"
        contact=""
        :sending="plugin.granted?.send ?? 'nothing'"
        :reminder="reminder"
        :session="session"
        fill
        @open-chat="(contact) => router.push(`/chat/${contact}`)"
        @done="router.back()"
        @closed="closed"
        @needs="(need) => (needing = need)"
        @pick="(ask) => (choosing = ask)"
        @send-to="sendTo"
      />
      <p v-else-if="ready" class="ft-plugin-page__missing">{{ $t("plugins.none") }}</p>
    </ion-content>
    <PermissionAsk
      :open="Boolean(needing)"
      :name="plugin ? pluginName(plugin) : ''"
      :permission="needing ?? undefined"
      :icon="plugin && pluginIcon(plugin)"
      :image="plugin && pluginImage(plugin)"
      @allow="answerNeed(true)"
      @cancel="answerNeed(false)"
    />
    <ContactPicker class="ft-sheet--window" :open="Boolean(choosing)" purpose="send" @pick="answerPick" @add="addContact" @dismiss="answerPick(null)" />
  </ion-page>
</template>

<style scoped>
.ft-plugin-page {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-plugin-page__lock {
  font-size: 40px;
}
.ft-plugin-page__missing {
  padding: var(--ft-space-4);
  color: var(--ft-muted);
  text-align: center;
}
</style>
