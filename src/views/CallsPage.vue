<script setup lang="ts">
import { onMounted } from "vue";
import { IonContent, IonHeader, IonIcon, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { arrowDownOutline, arrowUpOutline, callOutline, videocamOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import { clock, hueOf } from "../core";
import { history, loadHistory, type CallEntry } from "../calls";

// Plan §66: the history is what happened on this phone, never invented.
const router = useRouter();
onMounted(() => void loadHistory());

const DIRECTION_ICON: Record<string, string> = {
  incoming: arrowDownOutline,
  missed: arrowDownOutline,
  outgoing: arrowUpOutline,
};

function direction(entry: CallEntry): "incoming" | "outgoing" | "missed" {
  if (entry.outgoing) return "outgoing";
  return entry.outcome === "missed" ? "missed" : "incoming";
}

function when(entry: CallEntry): string {
  const date = new Date(entry.startedAt);
  const today = new Date().toDateString() === date.toDateString();
  return today ? clock(entry.startedAt) : date.toLocaleDateString(undefined, { day: "numeric", month: "short" });
}

function duration(seconds: number): string {
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

function callBack(entry: CallEntry) {
  void router.push(`/call/${entry.contact}${entry.video ? "?video=1" : ""}`);
}
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

      <p v-if="!history.calls.length" class="ft-empty" data-test="empty">{{ $t("calls.empty") }}</p>

      <ul v-else class="ft-rows">
        <li
          v-for="entry in history.calls"
          :key="entry.id"
          class="ft-call"
          :class="{ 'is-missed': direction(entry) === 'missed' }"
          data-test="call-row"
        >
          <Avatar :name="entry.name" :hue="hueOf(entry.contact)" :size="44" />
          <span class="ft-call__body">
            <span class="ft-call__name">{{ entry.name }}</span>
            <span class="ft-call__meta">
              <ion-icon
                :icon="DIRECTION_ICON[direction(entry)]"
                class="ft-call__direction"
                :aria-label="$t(`calls.${direction(entry)}`)"
                role="img"
              />
              <ion-icon
                :icon="entry.video ? videocamOutline : callOutline"
                :aria-label="entry.video ? $t('chat.videoCall') : $t('chat.voiceCall')"
                role="img"
              />
              <span>
                {{ entry.video ? $t("calls.video") : $t("calls.voice") }} · {{ when(entry)
                }}<template v-if="entry.seconds"> · {{ duration(entry.seconds) }}</template>
              </span>
            </span>
          </span>
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('calls.callBack')" @click="callBack(entry)">
            <ion-icon :icon="entry.video ? videocamOutline : callOutline" aria-hidden="true" />
          </button>
        </li>
      </ul>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-empty {
  margin: 20vh 0 0;
  color: var(--ft-muted);
  text-align: center;
}

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
