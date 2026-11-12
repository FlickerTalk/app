<script setup lang="ts">
import { IonButton, IonDatetime, IonText } from "@ionic/vue";
import { i18n } from "../i18n";

defineProps<{ presented: boolean; sendAt: string; soonest: string; latest: string; weekStart: number; canSchedule: boolean }>();
const emit = defineEmits<{ pick: [value: unknown]; cancel: []; schedule: [] }>();
</script>

<template>
  <div class="ion-padding ft-schedule" data-test="schedule">
    <h2 class="ft-schedule__title">{{ $t("chat.sendLater") }}</h2>
    <!-- Ionic writes «Time» unless given a label -->
    <div class="ft-schedule__room" data-test="schedule-room">
      <ion-datetime
        v-if="presented"
        class="ft-schedule__picker"
        presentation="date-time"
        :locale="i18n.global.locale.value"
        :first-day-of-week="weekStart"
        :model-value="sendAt"
        :min="soonest"
        :max="latest"
        data-test="schedule-at"
        @update:model-value="emit('pick', $event)"
      >
        <span slot="time-label">{{ $t("chat.scheduleTime") }}</span>
      </ion-datetime>
    </div>
    <p class="ft-schedule__note"><ion-text color="medium">{{ $t("chat.scheduleNote") }}</ion-text></p>
    <div class="ft-schedule__actions">
      <ion-button fill="outline" shape="round" data-test="schedule-cancel" @click="emit('cancel')">{{ $t("common.cancel") }}</ion-button>
      <ion-button shape="round" data-test="schedule-go" :disabled="!canSchedule" @click="emit('schedule')">{{ $t("chat.schedule") }}</ion-button>
    </div>
  </div>
</template>

<style scoped>
.ft-schedule {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-2);
  /* Ionic's iOS calendar is 350 px for every month */
  --ft-schedule-picker-height: 350px;
  padding-block-end: calc(var(--ion-padding, 16px) + var(--ion-safe-area-bottom, 0px));
}
.ft-schedule__room {
  min-height: var(--ft-schedule-picker-height);
}
.ft-schedule__title {
  margin: 0;
  font-size: 17px;
}
.ft-schedule__note {
  margin: 0;
  font-size: var(--ft-font-meta);
  text-align: center;
}
.ft-schedule__actions {
  display: flex;
  justify-content: center;
  flex-wrap: wrap;
  gap: var(--ft-space-2);
}
.ft-schedule__actions ion-button {
  text-transform: none;
}
</style>
