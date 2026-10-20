<script setup lang="ts">
/**
 * The frames that run a conversation's hooks out of sight (2026-10-20, plan of the translator
 * §8.1): one per tool, as `useChatHooks` asks for them, shared by a conversation, the contact's page
 * and a circle's. Each is laid out as the tool's window, so one that asks the user something
 * (`ft.show`) shows where it is (a frame moved would reload), with a way out; otherwise it is there,
 * neither seen nor touched. A permission a tool needs is asked on the spot, named after it.
 */
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

/**
 * `hooks`: the conversation's (`useChatHooks`). `contact` and `session`: handed to each frame, as a
 * window opened in that conversation gets them. `active`: whether the page holding them is the one
 * on screen, for Android's Back. `paneClass`: the pane the permission sheet covers on a wide screen.
 */
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

/** The tools with a frame here, as installed now. */
const tools = computed(() =>
  props.hooks.frames.value.map((id) => installed.value.find((one) => one.id === id)).filter((one): one is PluginView => Boolean(one)),
);
const seen = (id: string) => props.hooks.shown.has(id);

/** A permission a hidden tool asks for, and which tool. */
const needing = ref<{ tool: PluginView; need: PermissionNeed } | null>(null);
function answer(allowed: boolean) {
  const asked = needing.value;
  needing.value = null;
  asked?.need.answer(allowed);
}

/** ✕ or Back on a tool that showed itself for a hook: its frame goes, the run comes back empty. */
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
    :open="Boolean(needing)"
    :name="needing ? pluginName(needing.tool) : ''"
    :permission="needing?.need"
    :icon="needing ? pluginIcon(needing.tool) : undefined"
    :image="needing ? pluginImage(needing.tool) : undefined"
    @allow="answer(true)"
    @cancel="answer(false)"
  />
</template>

<style scoped>
/* As the conversation's tool window: the whole screen, under the status bar. */
.ft-app {
  position: fixed;
  inset: 0;
  z-index: 25;
  display: flex;
  flex-direction: column;
  background: var(--ft-bg);
  padding-top: var(--ion-safe-area-top, 0px);
}
/* Out of sight: there, laid out, neither seen nor touched. */
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
