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
import { extensionPuzzleOutline, globeOutline, chatbubbleEllipsesOutline, createOutline, trashOutline } from "ionicons/icons";
import { grantPlugin, plugins, removePlugin, type PluginPermissions, type PluginView } from "../core";
import { t } from "../i18n";

// Plan §53: a plugin is granted nothing by installing. Every permission it asked for is shown on
// its own, with a switch, and can be taken back at any time.
const installed = ref<PluginView[]>([]);
const asksToRemove = ref("");

onMounted(refresh);

async function refresh() {
  installed.value = await plugins();
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
