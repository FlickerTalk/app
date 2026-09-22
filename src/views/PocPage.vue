<script setup lang="ts">
// PoC 0 (Plan §87): connects to another peer through the signalling relay and says hello over
// WebRTC. Temporary developer screen; the real app connects through push and ft-core.
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type State = "idle" | "connecting" | "waiting" | "open" | "closed";

const relay = ref("");
const room = ref("demo");
const state = ref<State>("idle");
const log = ref<string[]>([]);
const stops: UnlistenFn[] = [];

const isOpen = computed(() => state.value === "open");

onMounted(async () => {
  relay.value = await invoke<string>("poc_default_relay");
  stops.push(await listen<string>("poc://state", (event) => (state.value = event.payload as State)));
  stops.push(await listen<string>("poc://message", (event) => log.value.push(`← ${event.payload}`)));
});

onBeforeUnmount(() => stops.forEach((stop) => stop()));

async function connect() {
  state.value = "connecting";
  try {
    await invoke("poc_connect", { relay: relay.value, room: room.value });
  } catch (error) {
    state.value = "closed";
    log.value.push(`✗ ${String(error)}`);
  }
}

async function sendHello() {
  await invoke("poc_send", { text: "hello" });
  log.value.push("→ hello");
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("poc.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-poc">
        <label class="ft-poc__field">
          <span>{{ $t("poc.relay") }}</span>
          <input v-model="relay" data-test="relay" autocapitalize="off" spellcheck="false" />
        </label>
        <label class="ft-poc__field">
          <span>{{ $t("poc.room") }}</span>
          <input v-model="room" data-test="room" autocapitalize="off" spellcheck="false" />
        </label>

        <div class="ft-poc__status" :class="`is-${state}`">
          <span v-if="isOpen" class="ft-spark" aria-hidden="true" />
          {{ $t(`poc.state.${state}`) }}
        </div>

        <div class="ft-poc__actions">
          <button
            type="button"
            class="ft-poc__button"
            data-test="connect"
            :disabled="state === 'connecting' || state === 'waiting' || isOpen"
            @click="connect"
          >
            {{ $t("poc.connect") }}
          </button>
          <button
            type="button"
            class="ft-poc__button ft-poc__button--accent"
            data-test="send"
            :disabled="!isOpen"
            @click="sendHello"
          >
            {{ $t("poc.sendHello") }}
          </button>
        </div>

        <ol class="ft-poc__log">
          <li v-for="(line, index) in log" :key="index">{{ line }}</li>
        </ol>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-poc {
  display: flex;
  flex-direction: column;
  gap: var(--ft-space-3);
  max-width: 560px;
  margin: 0 auto;
  padding: var(--ft-space-4);
}

.ft-poc__field {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-poc__field input {
  padding: 12px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 12px;
  background: var(--ft-surface);
  color: var(--ft-text);
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
}

.ft-poc__status {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 12px 14px;
  border-radius: 12px;
  background: var(--ft-surface-2);
  color: var(--ft-muted);
}
.ft-poc__status.is-open {
  color: var(--ft-accent);
}
.ft-poc__status.is-closed {
  color: var(--ion-color-danger);
}

.ft-poc__actions {
  display: flex;
  gap: var(--ft-space-3);
}
.ft-poc__button {
  appearance: none;
  flex: 1;
  padding: 13px;
  border: 1px solid var(--ft-border);
  border-radius: 999px;
  background: var(--ft-surface);
  color: var(--ft-text);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-poc__button--accent {
  border: 0;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
}
.ft-poc__button:disabled {
  opacity: 0.4;
  cursor: default;
}

.ft-poc__log {
  margin: 0;
  padding: 0;
  list-style: none;
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
  line-height: 1.8;
}
</style>
