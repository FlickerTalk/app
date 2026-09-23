<script setup lang="ts">
import { ref } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonIcon, IonPage, IonToolbar } from "@ionic/vue";
import { backspaceOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import { openSession } from "../core";

// Hidden sessions: six digits on a pad of its own, no keyboard, no name, no title. The sixth
// digit opens the session that has this PIN or a new empty one, and nothing says which.
const PIN_LENGTH = 6;
const KEYS = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "", "0", "delete"] as const;

const router = useRouter();
const pin = ref("");
const busy = ref(false);

async function press(key: string) {
  if (busy.value) return;
  if (key === "delete") {
    pin.value = pin.value.slice(0, -1);
    return;
  }
  if (pin.value.length >= PIN_LENGTH) return;
  pin.value += key;
  if (pin.value.length === PIN_LENGTH) await go();
}

async function go() {
  busy.value = true;
  try {
    await openSession(pin.value);
    router.push("/tabs/chats");
  } finally {
    pin.value = "";
    busy.value = false;
  }
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-pin">
        <div class="ft-pin__dots" role="status" :aria-label="$t('session.pin')">
          <span
            v-for="slot in PIN_LENGTH"
            :key="slot"
            class="ft-pin__dot"
            :class="{ 'is-filled': slot <= pin.length }"
            data-test="dot"
          />
        </div>
        <div class="ft-pin__pad">
          <template v-for="(key, index) in KEYS" :key="index">
            <span v-if="key === ''" aria-hidden="true" />
            <button
              v-else-if="key === 'delete'"
              type="button"
              class="ft-pin__key ft-pin__key--ghost"
              data-test="key-delete"
              :aria-label="$t('session.delete')"
              @click="press('delete')"
            >
              <ion-icon :icon="backspaceOutline" aria-hidden="true" />
            </button>
            <button v-else type="button" class="ft-pin__key" :data-test="`key-${key}`" @click="press(key)">
              {{ key }}
            </button>
          </template>
        </div>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-pin {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 36px;
  min-height: 100%;
  padding: var(--ft-space-5);
}
.ft-pin__dots {
  display: flex;
  gap: 18px;
}
.ft-pin__dot {
  width: 14px;
  height: 14px;
  border: 2px solid var(--ft-muted);
  border-radius: 50%;
  transition:
    background 0.12s,
    border-color 0.12s;
}
.ft-pin__dot.is-filled {
  border-color: var(--ft-accent);
  background: var(--ft-accent);
  box-shadow: 0 0 10px var(--ft-glow);
}
.ft-pin__pad {
  display: grid;
  grid-template-columns: repeat(3, 76px);
  gap: 16px;
}
.ft-pin__key {
  appearance: none;
  display: grid;
  place-items: center;
  width: 76px;
  height: 76px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: var(--ft-surface-2);
  color: var(--ft-text);
  font: inherit;
  font-size: 28px;
  font-weight: 500;
  cursor: pointer;
  transition: transform 0.1s ease;
}
.ft-pin__key:active {
  transform: scale(0.94);
  background: color-mix(in srgb, var(--ft-accent) 25%, var(--ft-surface-2));
}
.ft-pin__key:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}
.ft-pin__key--ghost {
  background: transparent;
  color: var(--ft-muted);
  font-size: 26px;
}
</style>
