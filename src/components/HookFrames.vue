<script setup lang="ts">
// frames sit where the tool's window would, moving one reloads it
import { computed, ref } from "vue";
import { IonButton, IonButtons, IonIcon, IonTitle, IonToolbar } from "@ionic/vue";
import { closeOutline } from "ionicons/icons";
import PluginSheet from "./PluginSheet.vue";
import PermissionAsk from "./PermissionAsk.vue";
import { installed, pluginIcon, pluginImage, pluginName } from "../plugins";
import { closeOnBackWhile } from "../back";
import type { ChatHooks, HookRunner } from "../chatHooks";
import type { PermissionNeed } from "../permissions";
import type { PluginView } from "../core";

const props = withDefaults(
  defineProps<{
    hooks: Pick<ChatHooks, "frames" | "shown" | "attach" | "setShown" | "dismiss">;
    contact: string;
    session?: string;
    active?: boolean;
    paneClass?: string;
  }>(),
  { session: undefined, active: true, paneClass: undefined },
);

const tools = computed(() =>
  props.hooks.frames.value.map((id) => installed.value.find((one) => one.id === id)).filter((one): one is PluginView => Boolean(one)),
);
const seen = (id: string) => props.hooks.shown.has(id);

const needing = ref<{ tool: PluginView; need: PermissionNeed } | null>(null);
function answer(allowed: boolean) {
  const asked = needing.value;
  needing.value = null;
  asked?.need.answer(allowed);
}

function dismissShown() {
  for (const id of [...props.hooks.shown]) props.hooks.dismiss(id);
}
closeOnBackWhile(() => props.active && Boolean(needing.value), () => answer(false));
closeOnBackWhile(() => props.active && props.hooks.shown.size > 0, dismissShown);
</script>

<template>
  <div
    v-for="one in tools"
    :key="one.id"
    class="ft-app"
    :class="{ 'is-unseen': !seen(one.id) }"
    role="dialog"
    :aria-label="pluginName(one)"
    :aria-hidden="!seen(one.id) || undefined"
    :inert="!seen(one.id) || undefined"
    :data-test="`hook-window-${one.id}`"
  >
    <ion-toolbar class="ft-app__bar">
      <ion-buttons slot="start">
        <ion-button data-test="close-hook" :aria-label="$t('common.back')" @click="hooks.dismiss(one.id)">
          <ion-icon slot="icon-only" :icon="closeOutline" aria-hidden="true" />
        </ion-button>
      </ion-buttons>
      <ion-title size="small"><span class="ft-title">{{ pluginName(one) }}</span></ion-title>
    </ion-toolbar>
    <div class="ft-app__body">
      <PluginSheet
        :ref="(frame) => hooks.attach(one.id, (frame as unknown as HookRunner | null) ?? null)"
        :plugin="one"
        :contact="contact"
        :sending="one.granted.send"
        :session="session"
        hidden
        fill
        @shown="(visible: boolean) => hooks.setShown(one.id, visible)"
        @needs="(need: PermissionNeed) => (needing = { tool: one, need })"
        @done="hooks.dismiss(one.id)"
      />
    </div>
  </div>
  <PermissionAsk
    :class="paneClass"
    :open="active && Boolean(needing)"
    :name="needing ? pluginName(needing.tool) : ''"
    :permission="needing?.need"
    :icon="needing ? pluginIcon(needing.tool) : undefined"
    :image="needing ? pluginImage(needing.tool) : undefined"
    @allow="answer(true)"
    @cancel="answer(false)"
  />
</template>

<style scoped>
.ft-app {
  position: fixed;
  inset: 0;
  z-index: 25;
  display: flex;
  flex-direction: column;
  background: var(--ft-bg);
  padding-top: var(--ion-safe-area-top, 0px);
}
.ft-app.is-unseen {
  visibility: hidden;
  pointer-events: none;
}
.ft-app__bar {
  flex: none;
}
.ft-app__body {
  flex: 1;
  min-height: 0;
  overflow: hidden;
  padding-bottom: var(--ion-safe-area-bottom, 0px);
  padding-left: var(--ion-safe-area-left, 0px);
  padding-right: var(--ion-safe-area-right, 0px);
}
</style>
