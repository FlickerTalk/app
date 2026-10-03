<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonBackButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonList,
  IonNote,
  IonPage,
  IonSpinner,
  IonTitle,
  IonToggle,
  IonToolbar,
} from "@ionic/vue";
import { addCircleOutline, downloadOutline, extensionPuzzleOutline, openOutline, trashOutline } from "ionicons/icons";
import {
  formatSize,
  grantPlugin,
  installPlugin,
  removePlugin,
  type OfferedPlugin,
  type PluginView,
} from "../core";
import { useRouter } from "vue-router";
import { isGame } from "../games";
import { permissionsOf, withPermission } from "../permissions";
import { byPluginName, pluginName, pluginSummary, refreshOffered, refreshPlugins, tools } from "../plugins";

const router = useRouter();

// Plan §53: a plugin is granted nothing by installing. Every permission it asked for is shown on
// its own, with a switch, and can be taken back at any time.
// The app's one list (2026-10-03): an update made in the background shows here at once.
const installed = computed(() => tools.value);
const offered = ref<OfferedPlugin[]>([]);
const asksToRemove = ref("");
/** The tool being installed now, and whether the last install failed (app#75). */
const installing = ref("");
const installFailed = ref(false);

onMounted(refresh);

// Plan 10.3: games have their own section; here, tools only.
// Each is named in the phone's language (2026-10-02): the catalogue is kept for every screen, so an
// installed tool whose package has no translation takes the catalogue's.
async function refresh() {
  await refreshPlugins();
  offered.value = byPluginName((await refreshOffered().catch(() => [])).filter((one) => !one.installed && !isGame(one)));
}

/** What a tool costs to bring in. The app already carries some of them: those cost nothing. */
function weight(one: OfferedPlugin): string {
  return one.carried ? "" : formatSize(one.size);
}

/** Nothing arrives installed: the user picks the tool, and it starts with no permission (§53). */
async function install(id: string) {
  installing.value = id;
  installFailed.value = false;
  try {
    await installPlugin(id);
  } catch {
    installFailed.value = true;
  } finally {
    installing.value = "";
  }
  await refresh();
}

async function toggle(plugin: PluginView, key: string, on: boolean) {
  await grantPlugin(plugin.id, withPermission(plugin, key, on));
  await refresh();
}

async function remove(id: string) {
  asksToRemove.value = "";
  await removePlugin(id);
  await refresh();
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ $t("plugins.title") }}</span></ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ft-plugins">
      <p class="ft-plugins__hint">{{ $t("plugins.hint") }}</p>

      <ion-list v-for="plugin in installed" :key="plugin.id" inset class="ft-group">
        <ion-item lines="none">
          <span slot="start" class="ft-tile"><ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" /></span>
          <ion-label>
            {{ pluginName(plugin) }}
            <p class="ft-muted">{{ plugin.version }}</p>
          </ion-label>
          <!-- 2026-09-27: a plugin can be opened on its own, with no chat behind it (notes, drive). -->
          <button
            slot="end"
            type="button"
            class="ft-plugins__remove ft-plugins__open"
            :data-test="`open-${plugin.id}`"
            :aria-label="$t('plugins.open')"
            @click="router.push(`/plugin/${plugin.id}`)"
          >
            <ion-icon :icon="openOutline" aria-hidden="true" />
          </button>
          <button
            v-if="asksToRemove !== plugin.id"
            slot="end"
            type="button"
            class="ft-plugins__remove"
            :data-test="`remove-${plugin.id}`"
            :aria-label="$t('plugins.remove')"
            @click="asksToRemove = plugin.id"
          >
            <ion-icon :icon="trashOutline" aria-hidden="true" />
          </button>
          <button
            v-else
            slot="end"
            type="button"
            class="ft-plugins__confirm"
            data-test="remove-confirm"
            @click="remove(plugin.id)"
          >
            {{ $t("plugins.remove") }}
          </button>
        </ion-item>

        <ion-item v-for="permission in permissionsOf(plugin)" :key="permission.key" lines="none">
          <span slot="start" class="ft-tile"><ion-icon :icon="permission.icon" aria-hidden="true" /></span>
          <ion-label>{{ permission.label }}</ion-label>
          <ion-toggle
            slot="end"
            :checked="permission.on"
            :aria-label="permission.label"
            @ion-change="toggle(plugin, permission.key, $event.detail.checked)"
          />
        </ion-item>
        <ion-item v-if="!permissionsOf(plugin).length" lines="none">
          <ion-note>{{ $t("plugins.asksNothing") }}</ion-note>
        </ion-item>
      </ion-list>

      <p v-if="!installed.length" class="ft-plugins__hint">{{ $t("plugins.none") }}</p>

      <!-- What this phone does not have yet: carried by the app (added, no size) or a download
           from the catalogue (§56). -->
      <template v-if="offered.length">
        <h2 class="ft-plugins__title">{{ $t("plugins.available") }}</h2>
        <ion-note v-if="installFailed" color="danger" class="ft-plugins__error" role="alert">{{ $t("plugins.installFailed") }}</ion-note>
        <ion-list inset class="ft-group">
          <ion-item v-for="one in offered" :key="one.id" lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ pluginName(one) }}
              <p class="ft-muted">{{ pluginSummary(one) }}<span v-if="weight(one)"> · {{ weight(one) }}</span></p>
            </ion-label>
            <button
              slot="end"
              type="button"
              class="ft-plugins__install"
              :data-test="`install-${one.id}`"
              :aria-label="$t('plugins.install')"
              :disabled="installing === one.id"
              :aria-busy="installing === one.id ? 'true' : undefined"
              @click="install(one.id)"
            >
              <ion-spinner v-if="installing === one.id" name="crescent" aria-hidden="true" />
              <ion-icon v-else :icon="one.carried ? addCircleOutline : downloadOutline" aria-hidden="true" />
            </button>
          </ion-item>
        </ion-list>
      </template>
    </ion-content>
  </ion-page>
</template>

<style scoped>
/* The last switch can be scrolled above Android's navigation bar (edge to edge). */
.ft-plugins {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-plugins__hint {
  margin: var(--ft-space-4);
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.4;
}
.ft-plugins__title {
  margin: var(--ft-space-5) var(--ft-space-4) 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--ft-muted);
}
.ft-plugins__error {
  display: block;
  margin: var(--ft-space-2) var(--ft-space-4) 0;
}
.ft-plugins__install {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--ft-accent);
  font-size: 20px;
  cursor: pointer;
}
.ft-plugins__install ion-spinner {
  width: 20px;
  height: 20px;
}
.ft-plugins__remove,
.ft-plugins__confirm {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--ion-color-danger);
  font: inherit;
  font-size: 15px;
  cursor: pointer;
}
.ft-plugins__confirm {
  font-weight: 600;
}
/* Open shares the remove button's reset; declared after it, so it keeps the accent, not the red. */
.ft-plugins__open {
  color: var(--ion-color-primary);
}
</style>
