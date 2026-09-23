<script setup lang="ts">
import { onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonList,
  IonPage,
  IonTitle,
  IonToggle,
  IonToolbar,
} from "@ionic/vue";
import { banOutline, sunnyOutline, timeOutline } from "ionicons/icons";
import { quietHours, setQuietHours, type Day, type Week } from "../core";

// Issue app#7: the weekly hours when the phone may make noise. Off by default; on, each day is
// all day, never, or a stretch. Outside them messages arrive in silence and calls do not ring.
// Kept on this phone only; the native side checks them against the phone's own clock.
const ALL_DAYS: Day[] = ["all", "all", "all", "all", "all", "all", "all"];
const STRETCH = { from: "09:00", to: "18:00" };

const week = ref<Week | null>(null);

onMounted(async () => {
  week.value = await quietHours();
});

async function save() {
  await setQuietHours(week.value);
}

async function turn(on: boolean) {
  week.value = on ? { days: [...ALL_DAYS] } : null;
  await save();
}

const mode = (day: Day) => (typeof day === "string" ? day : "hours");

async function choose(index: number, next: "all" | "none" | "hours") {
  if (!week.value || mode(week.value.days[index]) === next) return;
  week.value.days[index] = next === "hours" ? { ...STRETCH } : next;
  await save();
}

async function stretch(index: number, end: "from" | "to", value: string) {
  const day = week.value?.days[index];
  if (!week.value || typeof day === "string" || !day || !/^\d{2}:\d{2}$/.test(value)) return;
  week.value.days[index] = { ...day, [end]: value };
  await save();
}

const MODES = [
  { id: "all", icon: sunnyOutline, label: "hours.all" },
  { id: "none", icon: banOutline, label: "hours.none" },
  { id: "hours", icon: timeOutline, label: "hours.stretch" },
] as const;
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("hours.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-hours">
        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <ion-toggle :checked="week !== null" :aria-label="$t('hours.on')" @ion-change="turn($event.detail.checked)">
              <span class="ft-hours__title">{{ $t("hours.on") }}</span>
              <span class="ft-hours__note">{{ $t("hours.note") }}</span>
            </ion-toggle>
          </ion-item>
        </ion-list>

        <ul v-if="week" class="ft-hours__days">
          <li v-for="(day, index) in week.days" :key="index" class="ft-hours__day" data-test="day">
            <span class="ft-hours__name">{{ $t(`hours.days.${index}`) }}</span>
            <span class="ft-hours__modes" role="radiogroup" :aria-label="$t(`hours.days.${index}`)">
              <button
                v-for="option in MODES"
                :key="option.id"
                type="button"
                role="radio"
                class="ft-hours__mode"
                :class="{ 'is-active': mode(day) === option.id }"
                :aria-checked="mode(day) === option.id"
                :aria-label="$t(option.label)"
                :title="$t(option.label)"
                :data-test="`mode-${option.id}`"
                @click="choose(index, option.id)"
              >
                <ion-icon :icon="option.icon" aria-hidden="true" />
              </button>
            </span>
            <span v-if="typeof day !== 'string'" class="ft-hours__stretch">
              <input
                type="time"
                data-test="from"
                :value="day.from"
                :aria-label="$t('hours.from')"
                @change="stretch(index, 'from', ($event.target as HTMLInputElement).value)"
              />
              <span aria-hidden="true">–</span>
              <input
                type="time"
                data-test="to"
                :value="day.to"
                :aria-label="$t('hours.to')"
                @change="stretch(index, 'to', ($event.target as HTMLInputElement).value)"
              />
            </span>
          </li>
        </ul>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-hours {
  max-width: 720px;
  margin: 0 auto;
  padding-bottom: var(--ft-space-5);
}
.ft-group {
  --ion-item-background: var(--ft-surface);
  border: 1px solid var(--ft-border);
}
.ft-hours__title {
  display: block;
}
.ft-hours__note {
  display: block;
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-hours__days {
  margin: 0 var(--ft-space-4);
  padding: 0;
  list-style: none;
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
}
.ft-hours__day {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px;
  padding: 12px 16px;
}
.ft-hours__day + .ft-hours__day {
  border-top: 1px solid var(--ft-border);
}
.ft-hours__name {
  flex: 1;
  min-width: 90px;
  font-weight: 600;
}
.ft-hours__modes {
  display: flex;
  gap: 2px;
  padding: 3px;
  border-radius: 12px;
  background: var(--ft-surface-2);
}
.ft-hours__mode {
  appearance: none;
  display: grid;
  place-items: center;
  width: 34px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--ft-muted);
  font-size: 17px;
  cursor: pointer;
}
.ft-hours__mode.is-active {
  background: var(--ft-surface);
  color: var(--ft-accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25);
}
.ft-hours__mode:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}
.ft-hours__stretch {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  justify-content: flex-end;
  color: var(--ft-muted);
}
.ft-hours__stretch input {
  padding: 8px 10px;
  border: 1px solid var(--ft-border);
  border-radius: 10px;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font: inherit;
  color-scheme: dark light;
}
</style>
