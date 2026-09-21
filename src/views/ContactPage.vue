<script setup lang="ts">
import { computed } from "vue";
import {
  IonBackButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonList,
  IonPage,
  IonToolbar,
} from "@ionic/vue";
import { banOutline, flagOutline, qrCodeOutline } from "ionicons/icons";
import { useRoute } from "vue-router";
import Avatar from "../components/Avatar.vue";
import data from "../mock/chats.json";

const route = useRoute();
const contact = computed(
  () => data.chats.find((chat) => chat.id === String(route.params.id)) ?? data.chats[0],
);

// Plan §29: every contact has a fingerprint both sides can compare in person.
// Placeholder until ft-identity computes the real one from the public keys.
const fingerprint = computed(() => {
  const source = `${contact.value.id}:${contact.value.name}`;
  const groups: string[] = [];
  for (let group = 0; group < 12; group += 1) {
    let value = 0x9e37 + group * 0x85eb;
    for (const char of source) {
      value = (value * 31 + char.charCodeAt(0) + group) % 0xffff;
    }
    groups.push(value.toString(16).padStart(4, "0"));
  }
  return groups.join(" ");
});
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/chats" :aria-label="$t('common.back')" />
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-contact">
        <Avatar
          :name="contact.name"
          :hue="contact.hue"
          :size="88"
          :connected="contact.connected"
        />
        <h1 class="ft-contact__name">{{ contact.name }}</h1>
        <span class="ft-contact__status" :class="{ 'is-direct': contact.connected }">
          {{ contact.connected ? $t("chat.direct") : $t("chat.notConnected") }}
        </span>

        <section class="ft-contact__card">
          <span class="ft-contact__label">{{ $t("contact.fingerprint") }}</span>
          <code class="ft-contact__fingerprint" data-test="fingerprint">{{ fingerprint }}</code>
          <span class="ft-contact__hint">{{ $t("contact.verifyHint") }}</span>
        </section>

        <ion-list inset class="ft-group">
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="qrCodeOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("contact.verify") }}</ion-label>
          </ion-item>
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="banOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("contact.block") }}</ion-label>
          </ion-item>
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile ft-tile--danger">
              <ion-icon :icon="flagOutline" aria-hidden="true" />
            </span>
            <ion-label color="danger">{{ $t("contact.report") }}</ion-label>
          </ion-item>
        </ion-list>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-contact {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  max-width: 560px;
  margin: 0 auto;
  padding: var(--ft-space-2) 0 var(--ft-space-5);
  text-align: center;
}
.ft-contact__name {
  margin: var(--ft-space-3) 0 0;
  font-size: 24px;
  font-weight: 700;
}
.ft-contact__status {
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-contact__status.is-direct {
  color: var(--ft-accent);
}

.ft-contact__card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: calc(100% - var(--ft-space-5));
  margin: var(--ft-space-4) 0 var(--ft-space-2);
  padding: var(--ft-space-4);
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background: var(--ft-surface);
}
.ft-contact__label,
.ft-contact__hint {
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-contact__fingerprint {
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
  line-height: 1.7;
  letter-spacing: 0.04em;
  overflow-wrap: anywhere;
}

.ft-group {
  width: 100%;
  --ion-item-background: var(--ft-surface);
  border: 1px solid var(--ft-border);
  text-align: start;
}
.ft-tile--danger {
  background: color-mix(in srgb, var(--ion-color-danger) 16%, transparent);
  color: var(--ion-color-danger);
}
</style>
