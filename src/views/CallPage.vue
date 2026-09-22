<script setup lang="ts">
import { computed, ref } from "vue";
import { IonContent, IonIcon, IonPage } from "@ionic/vue";
import {
  callOutline,
  micOffOutline,
  micOutline,
  videocamOffOutline,
  videocamOutline,
  volumeHighOutline,
} from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import { chat } from "../core";

const route = useRoute();
const router = useRouter();

const contact = computed(() => chat(String(route.params.id)) ?? { name: "", hue: 0, connected: false });
const isVideo = computed(() => Boolean(route.query.video));

const muted = ref(false);
const cameraOff = ref(false);

// Plan §66: calls are always peer to peer; the relay only steps in per the user's setting (§17).
const state = computed(() => (contact.value.connected ? "ringing" : "connecting"));
</script>

<template>
  <ion-page>
    <ion-content class="ft-call">
      <div class="ft-call__body" :class="{ 'is-video': isVideo }">
        <div v-if="isVideo" class="ft-call__video" data-test="video">
          <span class="ft-call__self" />
        </div>

        <div class="ft-call__peer">
          <Avatar v-if="!isVideo" :name="contact.name" :hue="contact.hue" :size="132" />
          <h1 class="ft-call__name">{{ contact.name }}</h1>
          <span class="ft-call__state">
            {{ state === "ringing" ? $t("calls.ringing") : $t("calls.connecting") }}
          </span>
        </div>

        <div class="ft-call__controls">
          <button
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': muted }"
            :aria-label="$t('calls.mute')"
            :aria-pressed="muted"
            @click="muted = !muted"
          >
            <ion-icon :icon="muted ? micOffOutline : micOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('calls.speaker')">
            <ion-icon :icon="volumeHighOutline" aria-hidden="true" />
          </button>
          <button
            v-if="isVideo"
            type="button"
            class="ft-round ft-round--ghost"
            :class="{ 'is-on': cameraOff }"
            :aria-label="$t('calls.camera')"
            :aria-pressed="!cameraOff"
            @click="cameraOff = !cameraOff"
          >
            <ion-icon :icon="cameraOff ? videocamOffOutline : videocamOutline" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="ft-round ft-call__hangup"
            :aria-label="$t('calls.hangUp')"
            @click="router.back()"
          >
            <ion-icon :icon="callOutline" aria-hidden="true" />
          </button>
        </div>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-call {
  --background: var(--ft-bg);
}
.ft-call__body {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  min-height: 100%;
  padding: calc(env(safe-area-inset-top) + var(--ft-space-5)) var(--ft-space-4)
    calc(env(safe-area-inset-bottom) + var(--ft-space-5));
}

.ft-call__video {
  position: absolute;
  inset: 0;
  background:
    radial-gradient(90% 60% at 50% 20%, color-mix(in srgb, var(--ft-accent) 18%, transparent), transparent 70%),
    var(--ft-surface-2);
}
.ft-call__self {
  position: absolute;
  right: var(--ft-space-4);
  bottom: calc(env(safe-area-inset-bottom) + 110px);
  width: 96px;
  height: 140px;
  border: 1px solid var(--ft-border);
  border-radius: 16px;
  background: var(--ft-surface);
  box-shadow: 0 12px 30px -16px rgba(0, 0, 0, 0.8);
}

.ft-call__peer {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-3);
  margin-top: 12vh;
  text-align: center;
}
.is-video .ft-call__peer {
  margin-top: 0;
  padding: 10px 18px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--ft-bg) 55%, transparent);
  backdrop-filter: blur(10px);
}
.ft-call__name {
  margin: 0;
  font-size: 26px;
  font-weight: 700;
}
.ft-call__state {
  color: var(--ft-muted);
  font-size: 14px;
}

.ft-call__controls {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--ft-space-4);
}
.ft-call__controls .ft-round {
  width: 58px;
  height: 58px;
  font-size: 26px;
}
.ft-round.is-on {
  background: var(--ft-text);
  color: var(--ft-bg);
}
.ft-call__hangup {
  background: var(--ion-color-danger);
  color: #fff;
  transform: rotate(135deg);
}
</style>
