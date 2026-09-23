<script setup lang="ts">
import { onMounted, ref } from "vue";
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
  IonTitle,
  IonToggle,
  IonToolbar,
} from "@ionic/vue";
import {
  extensionPuzzleOutline,
  globeOutline,
  chatbubbleEllipsesOutline,
  createOutline,
  downloadOutline,
  trashOutline,
} from "ionicons/icons";
import {
  grantPlugin,
  installPlugin,
  offeredPlugins,
  plugins,
  removePlugin,
  type OfferedPlugin,
  type PluginPermissions,
  type PluginView,
} from "../core";
import { t } from "../i18n";

// Plan §53: a plugin is granted nothing by installing. Every permission it asked for is shown on
// its own, with a switch, and can be taken back at any time.
const installed = ref<PluginView[]>([]);
const offered = ref<OfferedPlugin[]>([]);
const asksToRemove = ref("");

onMounted(refresh);

async function refresh() {
  installed.value = await plugins();
  offered.value = (await offeredPlugins().catch(() => [])).filter((one) => !one.installed);
}

/** Nothing arrives installed: the user picks the tool, and it starts with no permission (§53). */
async function install(id: string) {
  await installPlugin(id);
  await refresh();
}

/** What a plugin asks for, one line per permission. */
function permissionsOf(plugin: PluginView) {
  const lines: { key: string; label: string; icon: string; on: boolean }[] = [];
  if (plugin.asks.messages) {
    lines.push({
      key: "messages",
      label: t("plugins.readsGiven"),
      icon: chatbubbleEllipsesOutline,
      on: plugin.granted.messages,
    });
  }
  for (const host of plugin.asks.network) {
    lines.push({
      key: `network:${host}`,
      label: host,
      icon: globeOutline,
      on: plugin.granted.network.includes(host),
    });
  }
  if (plugin.asks.send !== "nothing") {
    lines.push({
      key: "send",
      label: plugin.asks.send === "auto" ? t("plugins.sendsAlone") : t("plugins.writes"),
      icon: createOutline,
      on: plugin.granted.send !== "nothing",
    });
  }
  return lines;
}

async function toggle(plugin: PluginView, key: string, on: boolean) {
  const granted: PluginPermissions = {
    network: [...plugin.granted.network],
    messages: plugin.granted.messages,
    send: plugin.granted.send,
  };
  if (key === "messages") granted.messages = on;
  else if (key === "send") granted.send = on ? plugin.asks.send : "nothing";
  else {
    const host = key.slice("network:".length);
    granted.network = on ? [...new Set([...granted.network, host])] : granted.network.filter((one) => one !== host);
  }
  await grantPlugin(plugin.id, granted);
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
        <ion-title>{{ $t("plugins.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <p class="ft-plugins__hint">{{ $t("plugins.hint") }}</p>

      <ion-list v-for="plugin in installed" :key="plugin.id" inset class="ft-group">
        <ion-item lines="none">
          <span slot="start" class="ft-tile"><ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" /></span>
          <ion-label>
            {{ plugin.name }}
            <p class="ft-muted">{{ plugin.version }}</p>
          </ion-label>
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

      <!-- The tools that travel with the app and this phone does not have yet (§52). -->
      <template v-if="offered.length">
        <h2 class="ft-plugins__title">{{ $t("plugins.available") }}</h2>
        <ion-list inset class="ft-group">
          <ion-item v-for="one in offered" :key="one.id" lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="extensionPuzzleOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ one.name }}
              <p class="ft-muted">{{ one.version }}</p>
            </ion-label>
            <button
              slot="end"
              type="button"
              class="ft-plugins__install"
              :data-test="`install-${one.id}`"
              :aria-label="$t('plugins.install')"
              @click="install(one.id)"
            >
              <ion-icon :icon="downloadOutline" aria-hidden="true" />
            </button>
          </ion-item>
        </ion-list>
      </template>
    </ion-content>
  </ion-page>
</template>

<style scoped>
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
.ft-plugins__install {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--ft-accent);
  font-size: 20px;
  cursor: pointer;
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
</style>
