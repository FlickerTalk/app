<script setup lang="ts">
import { computed, ref, shallowRef, watch } from "vue";
import { IonButton, IonContent, IonIcon, IonItem, IonLabel, IonList, IonModal, IonNote, IonToggle } from "@ionic/vue";
import { addOutline, downloadOutline, lockClosedOutline, openOutline, play, trashOutline } from "ionicons/icons";
import { formatSize, type OfferedPlugin, type PluginView } from "../core";
import { isGame } from "../games";
import { permissionsOf } from "../permissions";
import { isLocked, offered, pluginIcon, pluginName, pluginSummary, premiumLocked } from "../plugins";

// 2026-10-08 (plan of the apps grid, screen 3): what an app is, from a hold on its tile. Installed:
// its permissions, one switch each (§53), Open or Play, and Remove, asked once. Not installed yet:
// what it does and weighs, and Install. The page does what the buttons ask.
const props = defineProps<{
  plugin: PluginView | OfferedPlugin | null;
  open: boolean;
  /** Installing it now: the button waits. */
  busy?: boolean;
}>();
const emit = defineEmits<{
  open: [];
  play: [];
  remove: [];
  install: [];
  toggle: [key: string, event: CustomEvent<{ checked: boolean }>];
  dismiss: [];
}>();

// What was shown stays on the sheet while it slides away, after the page lets go of it.
const shown = shallowRef(props.plugin);
watch(
  () => props.plugin,
  (plugin) => {
    if (plugin) shown.value = plugin;
  },
);

/** Removing is asked once, in place; a new opening asks again. */
const asksToRemove = ref(false);
watch(
  () => props.open,
  (open) => {
    if (open) asksToRemove.value = false;
  },
);

const here = computed(() => (shown.value && "asks" in shown.value ? shown.value : null));
const game = computed(() => Boolean(shown.value && isGame(shown.value)));
const locked = computed(() => Boolean(shown.value && isLocked(shown.value)));

/** What it weighs: a download's size; for an installed one, what the catalogue says, if it does. */
const size = computed(() => {
  const one = shown.value;
  if (!one) return "";
  const listed = "size" in one ? one : offered.value.find((entry) => entry.id === one.id);
  return listed && !listed.carried && listed.size > 0 ? formatSize(listed.size) : "";
});
const meta = computed(() => [shown.value?.version, size.value].filter(Boolean).join(" · "));
const summary = computed(() => (shown.value ? pluginSummary(shown.value) : ""));
const permissions = computed(() => (here.value ? permissionsOf(here.value) : []));
/** Installing a tool while the tools are locked offers the subscription instead (the page does it). */
const installLocked = computed(() => premiumLocked.value && !game.value);
const carried = computed(() => Boolean(shown.value && "carried" in shown.value && shown.value.carried));

function dismissed() {
  asksToRemove.value = false;
  emit("dismiss");
}
</script>

<template>
  <!-- Ionic takes the modal's name once, when it is made: a new one for each app shown. -->
  <ion-modal
    :key="shown?.id"
    :is-open="open"
    class="ft-app-sheet ft-sheet--tab"
    :breakpoints="[0, 0.6, 1]"
    :initial-breakpoint="0.6"
    :expand-to-scroll="false"
    :aria-label="shown ? pluginName(shown) : ''"
    @did-dismiss="dismissed"
  >
    <ion-content v-if="shown" class="ft-app-sheet__content">
      <div class="ft-app-sheet__body" data-test="app-sheet">
        <div class="ft-app-sheet__head">
          <span class="ft-app-sheet__icon"><ion-icon :icon="pluginIcon(shown)" aria-hidden="true" /></span>
          <h2 dir="auto">{{ pluginName(shown) }}</h2>
          <ion-note data-test="sheet-meta">{{ meta }}</ion-note>
        </div>
        <p v-if="summary" class="ft-app-sheet__summary" dir="auto" data-test="sheet-summary">{{ summary }}</p>

        <ion-list v-if="here" inset class="ft-app-sheet__permissions">
          <ion-item v-for="permission in permissions" :key="permission.key" lines="none">
            <ion-icon slot="start" :icon="permission.icon" aria-hidden="true" />
            <ion-label class="ion-text-wrap">{{ permission.label }}</ion-label>
            <ion-toggle
              slot="end"
              :checked="permission.on"
              :aria-label="permission.label"
              :data-test="`toggle-${permission.key}`"
              @ion-change="emit('toggle', permission.key, $event)"
            />
          </ion-item>
          <ion-item v-if="!permissions.length" lines="none">
            <ion-note>{{ $t("plugins.asksNothing") }}</ion-note>
          </ion-item>
        </ion-list>

        <!-- Removing a game deletes its saved games: said before, with a way back (as before). -->
        <div v-if="here && asksToRemove" class="ft-app-sheet__ask" role="alert">
          <p>{{ game ? $t("games.removeWarning") : $t("apps.removeAsk") }}</p>
          <div class="ft-app-sheet__actions">
            <ion-button fill="outline" data-test="remove-cancel" @click="asksToRemove = false">{{ $t("common.cancel") }}</ion-button>
            <ion-button color="danger" data-test="remove-confirm" @click="emit('remove')">{{ $t("apps.remove") }}</ion-button>
          </div>
        </div>

        <div v-else class="ft-app-sheet__actions">
          <template v-if="here">
            <ion-button v-if="game" expand="block" data-test="sheet-play" @click="emit('play')">
              <ion-icon slot="start" :icon="play" aria-hidden="true" />
              {{ $t("games.play") }}
            </ion-button>
            <ion-button v-else expand="block" data-test="sheet-open" @click="emit('open')">
              <ion-icon slot="start" :icon="locked ? lockClosedOutline : openOutline" aria-hidden="true" />
              {{ locked ? $t("apps.subscribe") : $t("apps.open") }}
            </ion-button>
            <ion-button fill="clear" color="danger" data-test="sheet-remove" @click="asksToRemove = true">
              <ion-icon slot="start" :icon="trashOutline" aria-hidden="true" />
              {{ $t("apps.remove") }}
            </ion-button>
          </template>
          <ion-button v-else expand="block" data-test="sheet-install" :disabled="busy" @click="emit('install')">
            <ion-icon slot="start" :icon="installLocked ? lockClosedOutline : carried ? addOutline : downloadOutline" aria-hidden="true" />
            {{ installLocked ? $t("apps.subscribe") : $t("apps.install") }}
          </ion-button>
        </div>
      </div>
    </ion-content>
  </ion-modal>
</template>

<style scoped>
/* The last button scrolls above Android's navigation bar (edge to edge), as the apps sheet's. */
.ft-app-sheet__content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
/* On a wide screen the sheet is wide: what it says keeps a phone's measure, centred. */
.ft-app-sheet__body {
  max-width: 560px;
  margin-inline: auto;
  padding: 26px 20px 20px;
}
.ft-app-sheet__head {
  display: flex;
  flex-direction: column;
  align-items: center;
  text-align: center;
}
.ft-app-sheet__head h2 {
  margin: 12px 0 2px;
  font-size: 22px;
  font-weight: 600;
}
.ft-app-sheet__head ion-note {
  font-size: 14px;
}
.ft-app-sheet__icon {
  display: grid;
  place-items: center;
  width: 72px;
  height: 72px;
  border-radius: 20px;
  background: rgba(var(--ion-color-primary-rgb), 0.16);
  color: var(--ion-color-primary);
}
.ft-app-sheet__icon ion-icon {
  font-size: 34px;
}
.ft-app-sheet__summary {
  margin: 12px 4px 4px;
  text-align: center;
  color: var(--ion-color-medium);
  font-size: 14px;
  line-height: 1.45;
}
/* A group of its own on the sheet, as in the mockup: a tint of Ionic's medium, rounded. */
.ft-app-sheet__permissions {
  --ion-item-background: transparent;
  margin: 14px 0 0;
  border-radius: 14px;
  background: rgba(var(--ion-color-medium-rgb), 0.12);
}
.ft-app-sheet__permissions ion-item ion-icon[slot="start"] {
  color: var(--ion-color-medium);
  margin-inline-end: 14px;
  font-size: 20px;
}
.ft-app-sheet__ask {
  margin-top: 16px;
  text-align: center;
}
.ft-app-sheet__ask p {
  margin: 0 0 4px;
  color: var(--ion-color-danger);
  font-size: 14px;
}
.ft-app-sheet__actions {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  margin-top: 16px;
}
.ft-app-sheet__actions ion-button {
  margin: 0;
  min-height: 46px;
  text-transform: none;
  letter-spacing: 0;
  font-weight: 600;
}
.ft-app-sheet__actions ion-button[expand="block"] {
  flex: 1;
}
</style>
