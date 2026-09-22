<script setup lang="ts">
import { computed } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import Avatar from "../components/Avatar.vue";
import { block, store } from "../core";

// Plan §35: blocking is local to this phone; this is where it is undone.
const blocked = computed(() => store.chats.filter((chat) => chat.blocked));
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("blocked.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <p v-if="!blocked.length" class="ft-empty" data-test="empty">{{ $t("blocked.empty") }}</p>
      <ul v-else class="ft-rows">
        <li v-for="chat in blocked" :key="chat.id" class="ft-blocked" data-test="blocked-row">
          <Avatar :name="chat.name" :hue="chat.hue" :size="44" />
          <span class="ft-blocked__name">{{ chat.name }}</span>
          <button type="button" class="ft-blocked__undo" :aria-label="$t('contact.unblock')" @click="block(chat.id, false)">
            {{ $t("contact.unblock") }}
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
.ft-blocked {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
}
.ft-blocked__name {
  flex: 1;
  font-size: 16px;
  font-weight: 600;
}
.ft-blocked__undo {
  padding: 8px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 999px;
  color: var(--ft-text);
  background: transparent;
}
</style>
