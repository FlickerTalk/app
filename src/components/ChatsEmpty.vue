<script setup lang="ts">
import { IonButton, IonIcon } from "@ionic/vue";
import { helpCircleOutline, qrCodeOutline } from "ionicons/icons";
import { useRouter } from "vue-router";

defineProps<{ compact: boolean }>();
const emit = defineEmits<{ tour: [] }>();

const router = useRouter();
const addContact = () => router.push("/add-contact");
</script>

<template>
  <div v-if="compact" class="ft-empty is-compact">
    <ion-button class="ft-empty__add" shape="round" data-test="empty-add" :aria-label="$t('addContact.title')" :title="$t('addContact.title')" @click="addContact">
      <ion-icon slot="icon-only" :icon="qrCodeOutline" aria-hidden="true" />
    </ion-button>
  </div>
  <div v-else class="ft-empty">
    <p class="ft-empty__title">{{ $t("chats.empty") }}</p>
    <p class="ft-empty__hint">{{ $t("chats.emptyHint") }}</p>
    <button type="button" class="ft-empty__action" @click="addContact">
      <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
      {{ $t("addContact.title") }}
    </button>
    <ion-button fill="clear" size="small" class="ft-empty__tour" data-test="tour-link" @click="emit('tour')">
      <ion-icon slot="start" :icon="helpCircleOutline" aria-hidden="true" />
      {{ $t("tour.link") }}
    </ion-button>
  </div>
</template>

<style scoped>
.ft-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-3);
  padding: 20vh var(--ft-space-5) 0;
  text-align: center;
}
.ft-empty.is-compact {
  padding: 16px 0 0;
}
.ft-empty__title {
  margin: 0;
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-empty__hint {
  margin: 0;
  max-width: 28ch;
  color: var(--ft-muted);
  line-height: 1.4;
}
.ft-empty__tour {
  text-transform: none;
}
.ft-empty__action {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-top: var(--ft-space-2);
  padding: 12px 20px;
  border: 0;
  border-radius: 999px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-empty__action:focus-visible,
.ft-empty__add.ion-focused {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}
.ft-empty__add {
  --background: var(--ft-surface-2);
  --color: var(--ft-accent);
  --box-shadow: none;
  --padding-start: 0;
  --padding-end: 0;
  width: 44px;
  height: 44px;
  margin: 0;
}
.ft-empty__add ion-icon {
  font-size: 20px;
}
</style>
