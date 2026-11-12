<script setup lang="ts">
import { ref } from "vue";
import { IonButton, IonIcon, IonSearchbar } from "@ionic/vue";
import { closeOutline } from "ionicons/icons";
import type { ChatMessage } from "../core";
import type { SearchField } from "../chatSearch";

defineProps<{ query: string; hits: ChatMessage[]; searched: boolean; contactName: string }>();
defineEmits<{ type: [value: unknown]; close: []; visit: [id: string] }>();

const searchbar = ref<{ $el: SearchField } | null>(null);
defineExpose({ searchbar });

const hitText = (message: ChatMessage) => (message.kind === "file" ? message.file?.name ?? "" : message.text);
</script>

<template>
  <div class="ft-search" data-test="search-panel">
    <div class="ft-search__row">
      <ion-searchbar
        ref="searchbar"
        class="ft-search__input"
        :model-value="query"
        data-test="search-input"
        :placeholder="$t('chat.searchIn')"
        :aria-label="$t('chat.searchIn')"
        enterkeyhint="search"
        @update:model-value="$emit('type', $event)"
      />
      <ion-button fill="clear" color="medium" data-test="close-search" :aria-label="$t('chat.closeSearch')" @click="$emit('close')">
        <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
      </ion-button>
    </div>
    <ul v-if="hits.length" class="ft-search__hits" data-test="search-hits">
      <li v-for="hit in hits" :key="hit.id">
        <button type="button" class="ft-search__hit" :data-test="`hit-${hit.id}`" @click="$emit('visit', hit.id)">
          <span class="ft-search__who">{{ hit.mine ? $t("chat.you") : contactName }} · {{ hit.time }}</span>
          <span class="ft-search__text" dir="auto">{{ hitText(hit) }}</span>
        </button>
      </li>
    </ul>
    <p v-else-if="searched" class="ft-search__none" data-test="search-none">{{ $t("chat.nothingFound") }}</p>
  </div>
</template>

<style scoped>
.ft-search {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding-block: 6px 8px;
  padding-inline: var(--ft-space-4);
  border-bottom: 1px solid var(--ion-border-color);
  background: var(--ion-color-light);
}
.ft-search__row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.ft-search__input {
  flex: 1;
  min-width: 0;
  padding: 0;
}
.ft-search__hits {
  max-height: 40vh;
  margin: 0;
  padding: 0;
  overflow: auto;
  list-style: none;
}
.ft-search__hit {
  appearance: none;
  display: flex;
  flex-direction: column;
  width: 100%;
  padding-block: 8px;
  padding-inline: 10px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--ion-text-color);
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.ft-search__hit:hover {
  background: var(--ion-color-light-shade);
}
.ft-search__who {
  font-size: var(--ft-font-meta);
  color: var(--ion-color-medium);
}
.ft-search__text {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-size: var(--ft-font-meta);
}
.ft-search__none {
  margin: 0;
  padding-block: 4px;
  padding-inline: 10px;
  color: var(--ion-color-medium);
  font-size: var(--ft-font-meta);
}
</style>
