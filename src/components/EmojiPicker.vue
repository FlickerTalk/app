<script setup lang="ts">
/** The emoji of the composer (issue app#4): tabs on top, a grid below, nothing downloaded. */
import { computed, ref } from "vue";
import { GROUPS, recent, remember } from "../emoji";
import { t } from "../i18n";

const emit = defineEmits<{ pick: [string] }>();

const lately = ref(recent());
const tabs = computed(() =>
  lately.value.length ? [{ id: "recent", icon: "🕘", emoji: lately.value }, ...GROUPS] : GROUPS,
);
const open = ref(tabs.value[0].id);
const shown = computed(() => tabs.value.find((tab) => tab.id === open.value) ?? tabs.value[0]);

function pick(emoji: string) {
  remember(emoji);
  emit("pick", emoji);
}
</script>

<template>
  <div class="ft-emoji" role="dialog" :aria-label="t('emoji.open')">
    <div class="ft-emoji__tabs" role="tablist">
      <button
        v-for="tab in tabs"
        :key="tab.id"
        type="button"
        class="ft-emoji__tab"
        :class="{ 'is-open': tab.id === open }"
        role="tab"
        :aria-selected="tab.id === open"
        :aria-label="t(`emoji.${tab.id}`)"
        :data-test="`group-${tab.id}`"
        @click="open = tab.id"
      >
        {{ tab.icon }}
      </button>
    </div>
    <div class="ft-emoji__grid">
      <button
        v-for="emoji in shown.emoji"
        :key="emoji"
        type="button"
        class="ft-emoji__one"
        :aria-label="emoji"
        data-test="emoji"
        @click="pick(emoji)"
      >
        {{ emoji }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.ft-emoji {
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--ft-border);
  background: var(--ft-surface);
}

.ft-emoji__tabs {
  display: flex;
  overflow-x: auto;
  gap: 2px;
  padding: 6px 8px;
  scrollbar-width: none;
}
.ft-emoji__tabs::-webkit-scrollbar {
  display: none;
}

.ft-emoji__tab {
  appearance: none;
  border: 0;
  background: transparent;
  border-radius: 10px;
  padding: 4px 8px;
  font-size: 20px;
  line-height: 1.2;
  cursor: pointer;
  opacity: 0.5;
}
.ft-emoji__tab.is-open {
  opacity: 1;
  background: var(--ft-surface-2);
}

.ft-emoji__grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(40px, 1fr));
  gap: 2px;
  padding: 0 6px 8px;
  max-height: 224px;
  overflow-y: auto;
}

.ft-emoji__one {
  appearance: none;
  border: 0;
  background: transparent;
  border-radius: 10px;
  padding: 4px 0;
  font-size: 26px;
  line-height: 1.3;
  cursor: pointer;
}
.ft-emoji__one:active {
  background: var(--ft-surface-2);
}
</style>
