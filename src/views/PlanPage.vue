<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { IonBackButton, IonButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar, toastController } from "@ionic/vue";
import { daysLeft, followPlan, payTrouble, plan as planOf, restoreSubscription, setAge, subscribe, subscriptionPrice, type PlanView } from "../core";
import { t } from "../i18n";

// Plan §40–§47: the first year is free from the install, then a yearly subscription at the
// Store's price (0,99 € in Spain since 2026-09-29); under 21 it is always free. Everything is
// decided on this phone, and no date of birth is ever kept (§30, §43).
const plan = ref<PlanView | null>(null);
const trouble = ref("");
/** What a year costs, as the Store formats it; null while unknown or when it cannot say. */
const price = ref<string | null>(null);

// The Store may change its mind with the screen open (2026-10-07): an approved Ask to Buy, the
// year running out, a renewal. The core says so and the screen reads the plan again.
let stopFollowing: (() => void) | null = null;
let gone = false;

onMounted(() => {
  void refresh();
  // The Store may take a while or not answer at all: the screen does not wait for it.
  void subscriptionPrice().then((said) => (price.value = said));
  void followPlan(() => void refresh()).then((stop) => (gone ? stop() : (stopFollowing = stop)));
});
onBeforeUnmount(() => {
  gone = true;
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
    case "young":
      return t("plan.young");
    case "subscribed":
      return t("plan.subscribed", { until: until.value });
    case "limited":
      return t("plan.limited");
    default:
      return "";
  }
});

/** The subscription is only asked of an adult whose free year is over (§42). */
const asksToPay = computed(() => plan.value?.state === "limited");

async function iAm(age: "minor" | "adult") {
  await setAge(age);
  await refresh();
}

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

      <div v-if="asksToPay" class="ft-plan__acts">
        <button type="button" class="ft-plan__pay" data-test="pay" @click="pay">{{ payText }}</button>
        <button type="button" class="ft-plan__young" data-test="young" @click="iAm('minor')">
          {{ $t("plan.iAmYoung") }}
        </button>
      </div>

      <ion-button v-if="asksToPay" fill="clear" class="ft-plan__restore" data-test="restore" @click="restore">
        {{ $t("plan.restore") }}
      </ion-button>

      <!-- Said once, kept as a word, and undone here if it was a mistake (§43). -->
      <button
        v-if="plan?.age === 'minor'"
        type="button"
        class="ft-plan__young"
        data-test="older"
        @click="iAm('adult')"
      >
        {{ $t("plan.iAmOlder") }}
      </button>

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
.ft-plan__young {
  appearance: none;
  border: 1px solid var(--ft-border);
  border-radius: 14px;
  padding: 12px 18px;
  margin: var(--ft-space-3) var(--ft-space-4) 0;
  background: transparent;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.ft-plan__acts .ft-plan__young {
  margin: 0;
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
