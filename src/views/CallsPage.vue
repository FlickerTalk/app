<script setup lang="ts">
import { IonContent, IonHeader, IonIcon, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { arrowDownOutline, arrowUpOutline, callOutline, videocamOutline } from "ionicons/icons";
import Avatar from "../components/Avatar.vue";
import data from "../mock/chats.json";

const DIRECTION_ICON: Record<string, string> = {
  incoming: arrowDownOutline,
  missed: arrowDownOutline,
  outgoing: arrowUpOutline,
};
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-title>{{ $t("tabs.calls") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <ion-header collapse="condense" class="ion-no-border">
        <ion-toolbar>
          <ion-title size="large">{{ $t("tabs.calls") }}</ion-title>
        </ion-toolbar>
      </ion-header>

      <ul class="ft-rows">
        <li
          v-for="call in data.calls"
          :key="call.id"
          class="ft-call"
          :class="{ 'is-missed': call.direction === 'missed' }"
          data-test="call-row"
        >
          <Avatar :name="call.name" :hue="call.hue" :size="44" />
          <span class="ft-call__body">
            <span class="ft-call__name">{{ call.name }}</span>
            <span class="ft-call__meta">
              <ion-icon
                :icon="DIRECTION_ICON[call.direction]"
                class="ft-call__direction"
                :aria-label="$t(`calls.${call.direction}`)"
                role="img"
              />
              <ion-icon
                :icon="call.kind === 'video' ? videocamOutline : callOutline"
                :aria-label="call.kind === 'video' ? $t('chat.videoCall') : $t('chat.voiceCall')"
                role="img"
              />
              <span>{{ call.kind === "video" ? $t("calls.video") : $t("calls.voice") }} · {{ call.time }}</span>
            </span>
          </span>
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('calls.callBack')">
            <ion-icon :icon="call.kind === 'video' ? videocamOutline : callOutline" aria-hidden="true" />
          </button>
        </li>
      </ul>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-call {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px;
}
.ft-call__body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.ft-call__name {
  font-size: 16px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.is-missed .ft-call__name {
  color: var(--ion-color-danger);
}
.ft-call__meta {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  color: var(--ft-muted);
}
.ft-call__direction {
  transform: rotate(45deg);
}
.is-missed .ft-call__direction {
  color: var(--ion-color-danger);
}
</style>
