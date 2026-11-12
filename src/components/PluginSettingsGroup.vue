<script setup lang="ts">
// a page left under others runs nothing, its permission sheet would cover the page on screen
import { computed, onMounted, ref, watch } from "vue";
import { IonIcon, IonItem, IonLabel, IonList, IonListHeader, IonNote, IonSelect, IonSelectOption, IonToggle } from "@ionic/vue";
import { hookIcon, hookLabel, settingsFields, settingsNote, type PluginHookResult } from "../plugins";
import type { ChatHooks, SettingsValues } from "../chatHooks";
import type { PluginView } from "../core";

const props = withDefaults(defineProps<{ plugin: PluginView; hooks: Pick<ChatHooks, "readSettings" | "settings">; active?: boolean }>(), { active: true });

const fields = computed(() => settingsFields(props.plugin));
const note = computed(() => settingsNote(props.plugin));
const values = ref<SettingsValues>({});
const options = ref<NonNullable<PluginHookResult["options"]>>({});
const ready = ref(false);
const busy = ref(false);

function show(result: PluginHookResult) {
  if (result.values) values.value = { ...result.values };
  if (result.options) options.value = result.options;
}

async function change(key: string, value: string | boolean) {
  if (!ready.value || busy.value || values.value[key] === value) return;
  const before = values.value;
  values.value = { ...before, [key]: value };
  busy.value = true;
  const result = await props.hooks.settings(props.plugin, { manual: true, values: values.value }).catch(() => null);
  busy.value = false;
  if (result) show(result);
  else values.value = before;
}

let asked = false;
async function read() {
  if (asked || !props.active) return;
  asked = true;
  const result = await props.hooks.readSettings(props.plugin).catch(() => null);
  if (!result) return;
  show(result);
  ready.value = true;
}
onMounted(read);
watch(() => props.active, read);
</script>

<template>
  <ion-list inset :data-test="`settings-${plugin.id}`">
    <ion-list-header class="ft-settings__header">
      <ion-icon :icon="hookIcon(plugin, 'settings')" class="ft-settings__icon" aria-hidden="true" />
      <ion-label>{{ hookLabel(plugin, "settings") }}</ion-label>
    </ion-list-header>
    <ion-item v-for="field in fields" :key="field.key" lines="none">
      <ion-select
        v-if="field.type === 'select'"
        :value="values[field.key]"
        interface="popover"
        :disabled="!ready || busy"
        :data-test="`setting-${field.key}`"
        @ion-change="change(field.key, $event.detail.value)"
      >
        <div slot="label">{{ field.label }}</div>
        <ion-select-option v-for="option in options[field.key] ?? []" :key="option.value" :value="option.value">{{ option.label }}</ion-select-option>
      </ion-select>
      <ion-toggle
        v-else
        :checked="values[field.key] === true"
        :disabled="!ready || busy"
        :data-test="`setting-${field.key}`"
        @ion-change="change(field.key, $event.detail.checked)"
      >
        {{ field.label }}
      </ion-toggle>
    </ion-item>
    <ion-item v-if="note" lines="none">
      <ion-note class="ft-settings__note">{{ note }}</ion-note>
    </ion-item>
  </ion-list>
</template>

<style scoped>
/* on iOS Ionic draws a list header as a large title with its content at the bottom */
.ft-settings__header {
  --color: var(--ion-text-color);
  align-items: center;
  min-height: 45px;
  font-size: 0.875rem;
  font-weight: 400;
}
.ft-settings__header ion-label {
  margin-block: 0;
}
.ft-settings__icon {
  margin-inline-end: 8px;
  font-size: 18px;
}
.ft-settings__note {
  font-size: var(--ft-font-meta);
  text-align: start;
}
</style>
