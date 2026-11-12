<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, watch } from "vue";
import { IonActionSheet, IonButton, IonContent, IonIcon, IonPage, IonSpinner, onIonViewWillLeave } from "@ionic/vue";
import { chevronDown, downloadOutline, easelOutline, stopCircleOutline, videocamOffOutline, videocamOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import CallControls from "../components/CallControls.vue";
import CallPeer from "../components/CallPeer.vue";
import CallStage from "../components/CallStage.vue";
import GamePermissions from "../components/GamePermissions.vue";
import PluginSheet from "../components/PluginSheet.vue";
import { closeOnBackWhile, goBack } from "../back";
import { useCallPresenting } from "../call-presenting";
import { callScreenMounted } from "../call-screen";
import { useCallVideo, type CallSurfaces } from "../call-video";
import { useOnScreen } from "../on-screen";
import { chat, sessionOf } from "../core";
import { pluginName, refreshPremiumLock } from "../plugins";
import { call, hangUp, startCall } from "../calls";

const route = useRoute();
const router = useRouter();

const id = computed(() => String(route.params.id));
const current = computed(() => call.contact === id.value && call.phase !== "idle");
const contact = computed(() => chat(id.value) ?? { name: "", hue: 0 });
const isVideo = computed(() => (current.value ? call.video : Boolean(route.query.video)));

const body = ref<HTMLElement | null>(null);
const surfaces = ref<CallSurfaces | null>(null);
// measured for the hole, so no CSS transform on it
const presentBox = ref<HTMLElement | null>(null);
const controls = ref<{ $el?: Element } | null>(null);
const minimize = ref<{ $el?: Element } | null>(null);
const peer = ref<{ $el?: Element } | null>(null);
const sheet = ref<InstanceType<typeof PluginSheet> | null>(null);

const onScreen = useOnScreen(() => cameIn());

const {
  presenting,
  presentPlugin,
  canOffer,
  choosing,
  starting,
  asking,
  declined,
  presentButtons,
  presentState,
  waitLabel,
  presentArea,
  onSheet,
  sheetLeaving,
  stop,
  answerAsk,
  closeOwnSheets,
  readPlugins,
  acceptPresented,
  presentRefused,
  presentDone,
  sheetClosed,
} = useCallPresenting({ id, contact, current, onScreen, router, sheet });

const {
  native,
  stage,
  showRemote,
  showLocal,
  localThumb,
  remoteThumb,
  invite,
  dark,
  cameraUsable,
  cameraRunning,
  unavailable,
  presentStyle,
  dragging,
  thumbStyle,
  camera,
  flip,
  grabThumb,
  moveThumb,
  dropThumb,
  cameIn,
  wentOut,
  attach,
  detach,
} = useCallVideo({ current, onScreen, presentArea, body, surfaces, presentBox, controls, minimize, peer });

let unmounted: (() => void) | undefined;
onMounted(() => {
  unmounted = callScreenMounted(router);
  void readPlugins();
  void refreshPremiumLock();
  if (!current.value || call.phase === "ended") void startCall(id.value, Boolean(route.query.video));
  attach();
});
let left = false;
let leaving: ReturnType<typeof setTimeout> | undefined;
onIonViewWillLeave(() => {
  clearTimeout(leaving);
  wentOut();
});
function leave(): Promise<unknown> | undefined {
  if (left) return;
  left = true;
  clearTimeout(leaving);
  return window.history.state?.back ? goBack(router) : router.replace(`/chat/${id.value}`);
}
closeOnBackWhile(() => onScreen.value, leave);
function end() {
  if (call.phase !== "idle" && call.phase !== "ended") void hangUp();
  void leave();
}

onUnmounted(() => {
  unmounted?.();
  detach();
  left = true;
  clearTimeout(leaving);
});
watch(
  () => call.phase,
  (phase) => {
    if (phase !== "active") closeOwnSheets();
    if (phase === "ended" && onScreen.value) leaving = setTimeout(leave, 1500);
  },
);
</script>

<template>
  <ion-page>
    <ion-content class="ft-call" :class="{ 'is-dark': dark }">
      <div
        ref="body"
        class="ft-call__body"
        :class="native ? { 'is-video': stage, 'is-live': stage } : { 'is-video': isVideo, 'is-live': Boolean(call.remote) }"
      >
        <ion-button
          v-if="call.phase !== 'ended'"
          ref="minimize"
          class="ft-call__minimize"
          fill="clear"
          shape="round"
          :aria-label="$t('calls.minimize')"
          @click="leave"
        >
          <ion-icon slot="icon-only" :icon="chevronDown" aria-hidden="true" />
        </ion-button>
        <ion-button
          v-if="canOffer"
          class="ft-call__end ft-call__end--clear"
          fill="clear"
          shape="round"
          data-test="present"
          :disabled="starting"
          :aria-label="$t('calls.present')"
          @click="choosing = !starting"
        >
          <ion-icon slot="icon-only" :icon="easelOutline" aria-hidden="true" />
        </ion-button>
        <ion-button
          v-else-if="presenting?.by === 'me'"
          class="ft-call__end"
          color="danger"
          shape="round"
          data-test="stop-presenting"
          :aria-label="$t('calls.stopPresenting')"
          @click="stop"
        >
          <ion-icon slot="icon-only" :icon="stopCircleOutline" aria-hidden="true" />
        </ion-button>
        <p v-if="presenting?.by === 'them'" class="ft-call__presenter" data-test="presenter" dir="auto">
          <ion-button
            v-if="presentState === 'declined'"
            fill="clear"
            size="small"
            color="medium"
            data-test="presenter-ask"
            :aria-label="$t('calls.presentAsk', { name: contact.name, plugin: presentPlugin ? pluginName(presentPlugin) : '' })"
            @click="declined = ''"
          >
            {{ $t("calls.presenting", { name: contact.name }) }}
          </ion-button>
          <template v-else>{{ $t("calls.presenting", { name: contact.name }) }}</template>
        </p>
        <section
          v-if="presentArea || onSheet"
          v-show="presentArea"
          ref="presentBox"
          class="ft-call__present"
          :class="{ 'is-dragging': dragging }"
          :style="presentStyle"
          data-test="present-area"
        >
          <PluginSheet
            v-if="onSheet"
            v-show="!sheetLeaving"
            ref="sheet"
            :key="`${onSheet.key}#${onSheet.round}`"
            :plugin="onSheet.plugin"
            :contact="id"
            :live="true"
            :file="onSheet.file"
            :session="sessionOf(id)"
            :presenting="onSheet.lead ? 'lead' : 'follow'"
            fill
            @done="presentDone"
            @refused="presentRefused"
            @closed="sheetClosed"
          />
          <div v-if="!onSheet || sheetLeaving" class="ft-call__present-wait">
            <ion-button v-if="presentState === 'accept'" shape="round" data-test="present-accept" @click="acceptPresented">
              <ion-icon slot="start" :icon="downloadOutline" aria-hidden="true" />
              {{ $t("calls.acceptToSee") }}
            </ion-button>
            <p v-else-if="presentState === 'failed'" class="ft-call__notice" role="alert" data-test="present-failed">{{ $t("calls.presentFailed") }}</p>
            <p v-else-if="presentState === 'tooBig'" class="ft-call__notice" role="alert" data-test="present-too-big">{{ $t("calls.presentTooBig") }}</p>
            <ion-spinner v-else name="crescent" color="medium" data-test="present-loading" role="status" :aria-label="waitLabel" />
          </div>
        </section>
        <CallStage
          ref="surfaces"
          :native="native"
          :is-video="isVideo"
          :stage="stage"
          :show-remote="showRemote"
          :show-local="showLocal"
          :remote-thumb="remoteThumb"
          :local-thumb="localThumb"
          :thumb-style="thumbStyle"
          @grab="grabThumb"
          @move="moveThumb"
          @let-go="dropThumb"
        />

        <CallPeer
          ref="peer"
          :name="contact.name"
          :hue="contact.hue"
          :avatar="native ? !stage : !isVideo || !call.remote"
          :style="presentArea ? { visibility: 'hidden' } : undefined"
        />

        <div class="ft-call__notices">
          <p v-if="call.cameraDenied" class="ft-call__notice" role="alert" data-test="camera-denied">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.cameraDenied") }}
          </p>
          <p v-if="call.cameraFailed" class="ft-call__notice" role="alert" data-test="camera-failed">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.cameraFailed") }}
          </p>
          <p v-if="unavailable" class="ft-call__notice" role="status">
            <ion-icon :icon="videocamOffOutline" aria-hidden="true" />
            {{ $t("calls.videoUnavailable") }}
          </p>
          <p v-if="invite && !presentArea" class="ft-call__notice ft-call__notice--invite">
            <ion-icon :icon="videocamOutline" aria-hidden="true" />
            {{ $t("calls.turnOnCamera") }}
          </p>
          <p v-if="presentState === 'missing'" class="ft-call__notice" role="status" data-test="present-missing">
            <ion-icon :icon="easelOutline" aria-hidden="true" />
            {{ $t("calls.presentMissing") }}
          </p>
        </div>

        <CallControls
          ref="controls"
          :native="native"
          :is-video="isVideo"
          :invite="invite"
          :camera-usable="cameraUsable"
          :camera-running="cameraRunning"
          @camera="camera"
          @flip="flip"
          @hang-up="end"
        />
      </div>
      <!-- Ionic moves an open sheet to the app root, so Vue must never use it as an anchor. The patch threw and the screen stopped redrawing. -->
      <ion-action-sheet :is-open="choosing" :header="$t('calls.present')" :buttons="presentButtons" @did-dismiss="choosing = false" />
      <GamePermissions
        :open="Boolean(asking)"
        :name="asking?.name ?? ''"
        :icon="asking?.icon"
        :image="asking?.image"
        :body="asking?.body"
        :allow-label="$t('calls.presentAllow')"
        @allow="answerAsk(true)"
        @cancel="answerAsk(false)"
      />
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-call {
  --background: var(--ft-bg);
}
.ft-call.is-dark {
  --ft-bg: #07090c;
  --ft-text: #f4f5f7;
  --ft-muted: rgba(244, 245, 247, 0.65);
  --ft-surface-2: rgba(255, 255, 255, 0.12);
  --background: #07090c;
  color: var(--ft-text);
}
.ft-call__body {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  min-height: 100%;
  padding: calc(env(safe-area-inset-top) + var(--ft-space-5)) var(--ft-space-4)
    calc(var(--ion-safe-area-bottom, 0px) + var(--ft-space-5));
}

.ft-call__minimize {
  position: absolute;
  z-index: 1;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline-start: calc(max(env(safe-area-inset-left), env(safe-area-inset-right)) + var(--ft-space-2));
  /* 44 pt, Apple's minimum tap target */
  min-width: 44px;
  min-height: 44px;
  /* a coloured clear button drops its background, and this one needs one over the pictures */
  --color: var(--ion-color-medium);
  --background: rgba(var(--ion-color-medium-rgb), 0.18);
}
.ft-call__end {
  position: absolute;
  z-index: 2;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline-end: calc(max(env(safe-area-inset-left), env(safe-area-inset-right)) + var(--ft-space-2));
  min-width: 44px;
  min-height: 44px;
}
.ft-call__end--clear {
  --color: var(--ion-color-medium);
  --background: rgba(var(--ion-color-medium-rgb), 0.18);
}
.ft-call__presenter {
  position: absolute;
  z-index: 2;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2));
  inset-inline: 60px;
  display: flex;
  align-items: center;
  justify-content: center;
  min-height: 44px;
  margin: 0;
  color: var(--ion-color-medium);
  font-size: 14px;
}
.ft-call__present {
  position: absolute;
  z-index: 1;
  inset-inline: 0;
  top: calc(env(safe-area-inset-top) + var(--ft-space-2) * 2 + 44px);
  bottom: calc(var(--ion-safe-area-bottom, 0px) + var(--ft-space-5) * 2 + 58px);
  overflow-y: auto;
  background: var(--ion-background-color);
}
.ft-call__present.is-dragging {
  pointer-events: none;
}
.ft-call__present-wait {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  padding: var(--ft-space-4);
}

.ft-call__notices {
  position: relative;
  z-index: 2;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-2);
  margin-top: auto;
  margin-bottom: var(--ft-space-4);
  max-width: 420px;
}
.ft-call__notice {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 8px 14px;
  border-radius: 16px;
  background: color-mix(in srgb, var(--ft-bg) 75%, transparent);
  backdrop-filter: blur(10px);
  font-size: 14px;
  text-align: start;
}
.ft-call__notice ion-icon {
  flex-shrink: 0;
  font-size: 20px;
}
.ft-call__notice--invite {
  color: var(--ft-accent);
}
</style>

<style>
/* native pictures sit under the WebView, nothing may paint behind the call screen */
html.ft-call-video,
html.ft-call-video body,
html.ft-call-video ion-app {
  background: transparent !important;
}
html.ft-call-video .ft-call {
  --background: transparent;
}
</style>
