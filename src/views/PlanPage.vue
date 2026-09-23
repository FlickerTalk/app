<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { payTrouble, plan as planOf, setAge, subscribe, type PlanView } from "../core";
import { t } from "../i18n";

// Plan §40–§47: the first year is free from the install, then 1 € a year; under 21 it is always
// free. Everything is decided on this phone, and no date of birth is ever kept (§30, §43).
const plan = ref<PlanView | null>(null);
const trouble = ref("");

onMounted(refresh);

async function refresh() {
  plan.value = await planOf().catch(() => null);
}

const days = computed(() => Math.max(0, Math.ceil(((plan.value?.until ?? 0) - Date.now()) / (24 * 3600 * 1000))));
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

/** The euro is only asked of an adult whose free year is over (§42). */
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
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("settings.plan") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <p class="ft-plan__where" data-test="where">{{ where }}</p>
      <p class="ft-plan__hint">{{ $t("plan.hint") }}</p>

      <div v-if="asksToPay" class="ft-plan__acts">
        <button type="button" class="ft-plan__pay" data-test="pay" @click="pay">{{ $t("plan.pay") }}</button>
        <button type="button" class="ft-plan__young" data-test="young" @click="iAm('minor')">
          {{ $t("plan.iAmYoung") }}
        </button>
      </div>

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
.ft-plan__trouble {
  margin: var(--ft-space-4);
  color: var(--ion-color-danger);
  font-size: 14px;
}
</style>
