<script setup lang="ts">
import { computed } from "vue";
import { IonIcon } from "@ionic/vue";
import { callOutline, videocamOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "./Avatar.vue";
import { chat, hueOf } from "../core";
import { acceptCall, call, hangUp } from "../calls";

// Plan §66: an incoming call rings over whatever screen is open.
const router = useRouter();
const ringing = computed(() => call.phase === "ringing");
const name = computed(() => chat(call.contact)?.name ?? call.contact.slice(0, 9));

async function answer() {
  const contact = call.contact;
  const accepting = acceptCall();
  await router.push(`/call/${contact}`);
  await accepting;
}
</script>

<template>
  <div v-if="ringing" class="ft-incoming" role="alertdialog" :aria-label="$t('calls.incoming')" data-test="incoming">
    <Avatar :name="name" :hue="hueOf(call.contact)" :size="52" />
    <span class="ft-incoming__body">
      <span class="ft-incoming__name">{{ name }}</span>
      <span class="ft-incoming__kind">
        <ion-icon
          :icon="call.video ? videocamOutline : callOutline"
          role="img"
          :aria-label="call.video ? $t('calls.video') : $t('calls.voice')"
        />
        {{ $t("calls.incoming") }}
      </span>
    </span>
    <button type="button" class="ft-round ft-incoming__decline" :aria-label="$t('calls.decline')" @click="hangUp">
      <ion-icon :icon="callOutline" aria-hidden="true" />
    </button>
    <button type="button" class="ft-round ft-incoming__answer" :aria-label="$t('calls.answer')" @click="answer">
      <ion-icon :icon="call.video ? videocamOutline : callOutline" aria-hidden="true" />
    </button>
  </div>
</template>

<style scoped>
.ft-incoming {
  position: fixed;
  z-index: 1000;
  top: calc(env(safe-area-inset-top) + 10px);
  left: 50%;
  display: flex;
  align-items: center;
  gap: 12px;
  width: min(calc(100% - 20px), 520px);
  padding: 12px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 24px;
  background: color-mix(in srgb, var(--ft-surface-2) 92%, transparent);
  backdrop-filter: blur(14px);
  box-shadow: 0 18px 40px -18px rgba(0, 0, 0, 0.8);
  transform: translateX(-50%);
  animation: ft-incoming-in 0.25s ease-out;
}
.ft-incoming__body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
}
.ft-incoming__name {
  font-size: 17px;
  font-weight: 700;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-incoming__kind {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--ft-muted);
  font-size: 13px;
}
.ft-incoming .ft-round {
  width: 48px;
  height: 48px;
  font-size: 22px;
  color: #fff;
}
.ft-incoming__decline {
  background: var(--ion-color-danger);
  transform: rotate(135deg);
}
.ft-incoming__answer {
  background: var(--ion-color-success);
  animation: ft-ring 1.2s ease-in-out infinite;
}
@keyframes ft-incoming-in {
  from {
    opacity: 0;
    transform: translate(-50%, -12px);
  }
}
@keyframes ft-ring {
  50% {
    box-shadow: 0 0 0 8px color-mix(in srgb, var(--ion-color-success) 25%, transparent);
  }
}
</style>
