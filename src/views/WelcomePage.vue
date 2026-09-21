<script setup lang="ts">
import { IonContent, IonIcon, IonPage } from "@ionic/vue";
import { arrowForward } from "ionicons/icons";
import { useRouter } from "vue-router";
import data from "../mock/chats.json";
import { setOnboarded } from "../preferences";

const router = useRouter();

// Plan §6: the identity is created on the device on first run. No account, no phone number.
function start() {
  setOnboarded();
  router.replace("/tabs/chats");
}
</script>

<template>
  <ion-page>
    <ion-content class="ft-welcome">
      <div class="ft-welcome__body">
        <span class="ft-welcome__mark" aria-hidden="true">
          <svg viewBox="0 0 32 32" width="64" height="64" fill="none">
            <circle cx="8" cy="24" r="4" fill="currentColor" opacity="0.55" />
            <circle cx="24" cy="8" r="4" fill="currentColor" />
            <path
              d="M11.5 20.5 20.5 11.5"
              stroke="currentColor"
              stroke-width="2.5"
              stroke-linecap="round"
              stroke-dasharray="3 4"
            />
          </svg>
        </span>

        <h1 class="ft-welcome__title">{{ $t("app.name") }}</h1>
        <p class="ft-welcome__tagline">{{ $t("app.tagline") }}</p>

        <section class="ft-welcome__identity">
          <span class="ft-welcome__label">{{ $t("welcome.identityCreated") }}</span>
          <code class="ft-welcome__id">{{ data.me.id }}</code>
          <span class="ft-welcome__note">{{ $t("welcome.noAccount") }}</span>
        </section>

        <button type="button" class="ft-welcome__start" data-test="start" @click="start">
          {{ $t("welcome.start") }}
          <ion-icon :icon="arrowForward" aria-hidden="true" />
        </button>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-welcome {
  --background: var(--ft-bg);
}
.ft-welcome__body {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--ft-space-4);
  min-height: 100%;
  padding: var(--ft-space-5);
  text-align: center;
}

.ft-welcome__mark {
  color: var(--ft-accent);
  filter: drop-shadow(0 0 16px var(--ft-glow));
}
.ft-welcome__title {
  margin: 0;
  font-size: 32px;
  font-weight: 700;
  letter-spacing: -0.02em;
}
.ft-welcome__tagline {
  margin: 0;
  max-width: 24ch;
  color: var(--ft-muted);
  line-height: 1.4;
}

.ft-welcome__identity {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
  max-width: 420px;
  margin-top: var(--ft-space-3);
  padding: var(--ft-space-4);
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background:
    radial-gradient(120% 140% at 0% 0%, color-mix(in srgb, var(--ft-accent) 14%, transparent), transparent 60%),
    var(--ft-surface);
}
.ft-welcome__label,
.ft-welcome__note {
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-welcome__id {
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 17px;
  overflow-wrap: anywhere;
}

.ft-welcome__start {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-top: var(--ft-space-3);
  padding: 14px 26px;
  border: 0;
  border-radius: 999px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font-size: 16px;
  font-weight: 600;
  cursor: pointer;
  box-shadow: 0 10px 26px -12px var(--ft-glow);
}
.ft-welcome__start:active {
  transform: scale(0.97);
}
</style>
