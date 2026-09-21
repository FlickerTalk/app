<script setup lang="ts">
import { ref } from "vue";
import {
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
  archiveOutline,
  banOutline,
  colorPaletteOutline,
  contrastOutline,
  copyOutline,
  eyeOffOutline,
  fileTrayOutline,
  informationCircleOutline,
  moonOutline,
  phonePortraitOutline,
  qrCodeOutline,
  sparklesOutline,
  sunnyOutline,
  swapHorizontalOutline,
} from "ionicons/icons";
import Avatar from "../components/Avatar.vue";
import data from "../mock/chats.json";
import {
  applyAppearance,
  applyDirection,
  storedAppearance,
  storedDirection,
  type Appearance,
  type Direction,
} from "../theme";

// The offline mailbox is on by default (Plan §19).
const mailbox = ref(true);

function onMailboxChange(event: CustomEvent<{ checked: boolean }>) {
  mailbox.value = event.detail.checked;
}

const colors: { id: Direction; label: string; swatch: string }[] = [
  { id: "ember", label: "Ember", swatch: "linear-gradient(135deg, #ffa24c, #ff6a3d)" },
  { id: "aurora", label: "Aurora", swatch: "linear-gradient(135deg, #3ddbc4, #7b7cff)" },
  { id: "mono", label: "Mono", swatch: "linear-gradient(135deg, #f5f5f7 50%, #c6f432 50%)" },
];
const appearances: { id: Appearance; label: string; icon: string }[] = [
  { id: "system", label: "System", icon: phonePortraitOutline },
  { id: "dark", label: "Dark", icon: moonOutline },
  { id: "light", label: "Light", icon: sunnyOutline },
];

const direction = ref(storedDirection());
const appearance = ref(storedAppearance());

function chooseColor(id: Direction) {
  direction.value = id;
  applyDirection(id);
}

function chooseAppearance(id: Appearance) {
  appearance.value = id;
  applyAppearance(id);
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-title>Settings</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <ion-header collapse="condense" class="ion-no-border">
        <ion-toolbar>
          <ion-title size="large">Settings</ion-title>
        </ion-toolbar>
      </ion-header>

      <div class="ft-settings">
        <section class="ft-me">
          <Avatar :name="data.me.name" :hue="data.me.hue" :size="60" />
          <span class="ft-me__text">
            <span class="ft-me__label">Your FlickerTalk ID</span>
            <code class="ft-me__id">{{ data.me.id }}</code>
          </span>
          <button type="button" class="ft-round ft-round--ghost" aria-label="Copy ID">
            <ion-icon :icon="copyOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-round--accent" aria-label="Show my QR">
            <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
          </button>
        </section>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="fileTrayOutline" aria-hidden="true" /></span>
            <ion-toggle :checked="mailbox" aria-label="Offline mailbox" @ion-change="onMailboxChange">
              <span class="ft-item__title">Offline mailbox</span>
              <span class="ft-item__note">Encrypted · deleted on pickup</span>
            </ion-toggle>
          </ion-item>
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="banOutline" aria-hidden="true" /></span>
            <ion-label>Blocked</ion-label>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="colorPaletteOutline" aria-hidden="true" /></span>
            <ion-label>Color</ion-label>
            <span slot="end" class="ft-choices" role="group" aria-label="Color">
              <button
                v-for="color in colors"
                :key="color.id"
                type="button"
                class="ft-swatch"
                :class="{ 'is-active': direction === color.id }"
                :style="{ background: color.swatch }"
                :aria-label="color.label"
                :aria-pressed="direction === color.id"
                :title="color.label"
                @click="chooseColor(color.id)"
              />
            </span>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="contrastOutline" aria-hidden="true" /></span>
            <ion-label>Appearance</ion-label>
            <span slot="end" class="ft-choices ft-segment" role="group" aria-label="Appearance">
              <button
                v-for="option in appearances"
                :key="option.id"
                type="button"
                class="ft-segment__option"
                :class="{ 'is-active': appearance === option.id }"
                :aria-label="option.label"
                :aria-pressed="appearance === option.id"
                :title="option.label"
                @click="chooseAppearance(option.id)"
              >
                <ion-icon :icon="option.icon" aria-hidden="true" />
              </button>
            </span>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <!-- Plan §60: a QR pairs both phones for a direct P2P transfer; it never contains the key. -->
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="swapHorizontalOutline" aria-hidden="true" /></span>
            <ion-label>Move to a new phone</ion-label>
          </ion-item>
          <!-- Plan §61: encrypted .ftbackup file, for when the old phone is lost. -->
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="archiveOutline" aria-hidden="true" /></span>
            <ion-label>Backup</ion-label>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="sparklesOutline" aria-hidden="true" /></span>
            <ion-label>Plan</ion-label>
            <ion-note slot="end">Free · first year</ion-note>
          </ion-item>
          <ion-item button detail lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="eyeOffOutline" aria-hidden="true" /></span>
            <ion-label>Privacy</ion-label>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="informationCircleOutline" aria-hidden="true" /></span>
            <ion-label>Version</ion-label>
            <ion-note slot="end">0.1.0</ion-note>
          </ion-item>
        </ion-list>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-settings {
  max-width: 720px;
  margin: 0 auto;
  padding-bottom: var(--ft-space-5);
}

.ft-me {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: var(--ft-space-2) var(--ft-space-4) var(--ft-space-3);
  padding: var(--ft-space-4);
  border: 1px solid var(--ft-border);
  border-radius: var(--ft-radius-card);
  background:
    radial-gradient(120% 140% at 0% 0%, color-mix(in srgb, var(--ft-accent) 14%, transparent), transparent 60%),
    var(--ft-surface);
}
.ft-me__text {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.ft-me__label {
  font-size: 12px;
  color: var(--ft-muted);
}
.ft-me__id {
  font-family: ui-monospace, "SF Mono", Menlo, monospace;
  font-size: 15px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.ft-group {
  --ion-item-background: var(--ft-surface);
  border: 1px solid var(--ft-border);
}

.ft-tile {
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  margin-inline-end: 12px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--ft-accent) 14%, transparent);
  color: var(--ft-accent);
  font-size: 18px;
}

.ft-choices {
  display: flex;
  align-items: center;
  gap: 10px;
}
.ft-swatch {
  width: 26px;
  height: 26px;
  padding: 0;
  border: 2px solid transparent;
  border-radius: 50%;
  box-shadow: 0 0 0 1px var(--ft-border);
  cursor: pointer;
  transition: transform 0.15s ease;
}
.ft-swatch.is-active {
  border-color: var(--ft-bg);
  box-shadow: 0 0 0 2px var(--ft-accent), 0 0 12px var(--ft-glow);
  transform: scale(1.08);
}
.ft-swatch:focus-visible,
.ft-segment__option:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}

.ft-segment {
  gap: 2px;
  padding: 3px;
  border-radius: 12px;
  background: var(--ft-surface-2);
}
.ft-segment__option {
  appearance: none;
  display: grid;
  place-items: center;
  width: 34px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 9px;
  background: transparent;
  color: var(--ft-muted);
  font-size: 17px;
  cursor: pointer;
  transition:
    background 0.15s,
    color 0.15s;
}
.ft-segment__option.is-active {
  background: var(--ft-surface);
  color: var(--ft-accent);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.25);
}

.ft-item__title {
  display: block;
}
.ft-item__note {
  display: block;
  font-size: 12px;
  color: var(--ft-muted);
}
</style>
