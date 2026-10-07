<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { IonBackButton, IonButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar, onIonViewWillEnter, toastController } from "@ionic/vue";
import { daysLeft, followPlan, payTrouble, plan as planOf, restoreSubscription, subscribe, subscriptionPrice, type PlanView } from "../core";
import { t } from "../i18n";

// Plan §40–§47 (Ioan, 2026-10-08): chat, calls, files and games are free forever. The premium part
// (the tools and the extra sessions with a PIN) is free for 15 days from the install, then a yearly
// subscription at the Store's price (0,99 € in Spain since 2026-09-29). There is no age rule.
// Everything is decided on this phone.
const plan = ref<PlanView | null>(null);
const trouble = ref("");
/** What a year costs, as the Store formats it; null while unknown or when it cannot say. */
const price = ref<string | null>(null);

// The Store may change its mind with the screen open (2026-10-07): an approved Ask to Buy, the
// year running out, a renewal. The core says so and the screen reads the plan again.
let stopFollowing: (() => void) | null = null;
let gone = false;

/**
 * Asks the Store what a year costs (2026-10-08: on entering, when the plan changes and back on the
 * screen, since the Store's account or country may have changed). The Store may take a while or
 * not answer at all: the screen does not wait for it, and keeps the last price it said.
 */
function askPrice() {
  void subscriptionPrice().then((said) => {
    if (said) price.value = said;
  });
}

function onVisible() {
  if (document.visibilityState !== "visible") return;
  void refresh();
  askPrice();
}

onMounted(() => {
  void refresh();
  askPrice();
  document.addEventListener("visibilitychange", onVisible);
  // What the Store said before (waiting for approval, say) is no longer true then.
  void followPlan(() => {
    trouble.value = "";
    void refresh();
    askPrice();
  }).then((stop) => (gone ? stop() : (stopFollowing = stop)));
});
// Ionic keeps the page alive in its stack: back on it, the plan and the price are read again.
onIonViewWillEnter(() => {
  void refresh();
  askPrice();
});
onBeforeUnmount(() => {
  gone = true;
  document.removeEventListener("visibilitychange", onVisible);
  stopFollowing?.();
});

/** With the Store's price, or with no amount at all: the app never writes one of its own. */
const payText = computed(() => (price.value ? t("plan.pay", { price: price.value }) : t("plan.payYearly")));
const hint = computed(() => (price.value ? t("plan.hint", { price: price.value }) : t("plan.hintYearly")));

async function refresh() {
  plan.value = await planOf().catch(() => null);
}

const days = computed(() => daysLeft(plan.value?.until ?? 0));
const until = computed(() => new Date(plan.value?.until ?? 0).toLocaleDateString());

/** What the screen says, in one line, about where the user stands. */
const where = computed(() => {
  switch (plan.value?.state) {
    case "trial":
      return t("plan.trial", { days: days.value });
    case "subscribed":
      return plan.value.renews ? t("plan.renewing") : t("plan.subscribed", { until: until.value });
    case "limited":
      return t("plan.limited");
    default:
      return "";
  }
});

/** The subscription is only asked once the free days are over (§42). */
const asksToPay = computed(() => plan.value?.state === "limited");

async function pay() {
  trouble.value = "";
  try {
    await subscribe();
  } catch (error) {
    const say = payTrouble(error);
    trouble.value = say ? t(say) : "";
  }
  await refresh();
}

/** A year bought on another phone, or before a reinstall, comes back from the Store (2026-10-07). */
async function restore() {
  trouble.value = "";
  try {
    const found = await restoreSubscription();
    const notice = await toastController.create({
      message: t(found === "restored" ? "plan.restored" : "plan.nothingToRestore"),
      duration: 2500,
      position: "bottom",
    });
    await notice.present();
  } catch (error) {
    const say = payTrouble(error);
    trouble.value = say ? t(say) : "";
  }
  await refresh();
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ $t("settings.plan") }}</span></ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <p class="ft-plan__where" data-test="where">{{ where }}</p>
      <p class="ft-plan__hint" data-test="hint">{{ hint }}</p>
      <p class="ft-plan__hint" data-test="premium">{{ $t("plan.premium") }}</p>

      <div v-if="asksToPay" class="ft-plan__acts">
        <button type="button" class="ft-plan__pay" data-test="pay" @click="pay">{{ payText }}</button>
      </div>

      <ion-button v-if="asksToPay" fill="clear" class="ft-plan__restore" data-test="restore" @click="restore">
        {{ $t("plan.restore") }}
      </ion-button>

      <p v-if="trouble" class="ft-plan__trouble" role="alert" data-test="trouble">{{ trouble }}</p>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-plan__where {
  margin: var(--ft-space-5) var(--ft-space-4) var(--ft-space-3);
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-plan__hint {
  margin: 0 var(--ft-space-4) var(--ft-space-5);
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.45;
}
.ft-plan__acts {
  display: flex;
  flex-wrap: wrap;
  gap: var(--ft-space-3);
  margin: 0 var(--ft-space-4);
}
.ft-plan__pay {
  appearance: none;
  border: 0;
  border-radius: 14px;
  padding: 12px 18px;
  background: var(--ft-accent);
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-plan__restore {
  margin: var(--ft-space-3) var(--ft-space-2) 0;
}
.ft-plan__trouble {
  margin: var(--ft-space-4);
  color: var(--ion-color-danger);
  font-size: 14px;
}
</style>
