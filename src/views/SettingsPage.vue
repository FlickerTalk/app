<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useRouter } from "vue-router";
import {
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonList,
  IonNote,
  IonPage,
  IonSelect,
  IonSelectOption,
  IonTitle,
  IonToggle,
  IonToolbar,
} from "@ionic/vue";
import {
  banOutline,
  callOutline,
  colorPaletteOutline,
  contrastOutline,
  copyOutline,
  fileTrayOutline,
  informationCircleOutline,
  moonOutline,
  phonePortraitOutline,
  qrCodeOutline,
  sparklesOutline,
  sunnyOutline,
  swapHorizontalOutline,
  trashOutline,
} from "ionicons/icons";
import Avatar from "../components/Avatar.vue";
import { erasePhone, setMailbox, store } from "../core";
import { setCallRouting, storedCallRouting, type CallRouting } from "../preferences";
import { t } from "../i18n";
import {
  applyAppearance,
  applyDirection,
  storedAppearance,
  storedDirection,
  type Appearance,
  type Direction,
} from "../theme";

const router = useRouter();

// §41: free for a year from the install, counted on this phone.
const plan = computed(() => {
  const days = Math.floor((store.me.freeUntil - Date.now()) / (24 * 3600 * 1000));
  if (!store.me.freeUntil) return t("settings.planFree");
  return days >= 0 ? t("settings.planFreeDays", { days }) : t("settings.planOver");
});

const version = ref("");
onMounted(async () => {
  version.value = await getVersion().catch(() => "");
});


// Plan §19: the preference lives in the core, which tells contacts; the server never stores it.
async function onMailboxChange(event: CustomEvent<{ checked: boolean }>) {
  await setMailbox(event.detail.checked);
}

const callRouting = ref(storedCallRouting());

function onCallRoutingChange(event: CustomEvent<{ value: CallRouting }>) {
  callRouting.value = event.detail.value;
  setCallRouting(callRouting.value);
}

const colors: { id: Direction; label: string; swatch: string }[] = [
  { id: "mono", label: t("colors.mono"), swatch: "linear-gradient(135deg, #ffffff 50%, #000000 50%)" },
  { id: "ember", label: t("colors.ember"), swatch: "linear-gradient(135deg, #ffa24c, #ff6a3d)" },
  { id: "aurora", label: t("colors.aurora"), swatch: "linear-gradient(135deg, #3ddbc4, #7b7cff)" },
];
const appearances: { id: Appearance; label: string; icon: string }[] = [
  { id: "system", label: t("settings.system"), icon: phonePortraitOutline },
  { id: "dark", label: t("settings.dark"), icon: moonOutline },
  { id: "light", label: t("settings.light"), icon: sunnyOutline },
];

const direction = ref(storedDirection());
const asksToErase = ref(false);

// §78: it takes this device off the router and wipes the phone, so it asks first.
async function erase() {
  asksToErase.value = false;
  await erasePhone();
}
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
        <ion-title>{{ $t("settings.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <ion-header collapse="condense" class="ion-no-border">
        <ion-toolbar>
          <ion-title size="large">{{ $t("settings.title") }}</ion-title>
        </ion-toolbar>
      </ion-header>

      <div class="ft-settings">
        <section class="ft-me">
          <Avatar :name="store.me.name || store.me.id" :hue="store.me.hue" :size="60" />
          <span class="ft-me__text">
            <span class="ft-me__label">{{ $t("settings.yourId") }}</span>
            <code class="ft-me__id">{{ store.me.id }}</code>
          </span>
          <button type="button" class="ft-round ft-round--ghost" :aria-label="$t('settings.copyId')">
            <ion-icon :icon="copyOutline" aria-hidden="true" />
          </button>
          <button type="button" class="ft-round ft-round--accent" :aria-label="$t('settings.showQr')">
            <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
          </button>
        </section>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="fileTrayOutline" aria-hidden="true" /></span>
            <ion-toggle :checked="store.me.mailbox" :aria-label="$t('settings.mailbox')" @ion-change="onMailboxChange">
              <span class="ft-item__title">{{ $t("settings.mailbox") }}</span>
              <span class="ft-item__note">{{ $t("settings.mailboxNote") }}</span>
            </ion-toggle>
          </ion-item>
          <!-- Plan §17/§67: "always" hides your IP from the contact; "direct" never uses our relay. -->
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="callOutline" aria-hidden="true" /></span>
            <ion-select
              :value="callRouting"
              :aria-label="$t('settings.calls')"
              :label="$t('settings.calls')"
              interface="action-sheet"
              @ion-change="onCallRoutingChange"
            >
              <ion-select-option value="direct">{{ $t("settings.callsDirect") }}</ion-select-option>
              <ion-select-option value="auto">{{ $t("settings.callsAuto") }}</ion-select-option>
              <ion-select-option value="always">{{ $t("settings.callsAlways") }}</ion-select-option>
            </ion-select>
          </ion-item>
          <ion-item button detail lines="none" data-test="blocked" @click="router.push('/blocked')">
            <span slot="start" class="ft-tile"><ion-icon :icon="banOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.blocked") }}</ion-label>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="colorPaletteOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.color") }}</ion-label>
            <span slot="end" class="ft-choices" role="group" :aria-label="$t('settings.color')">
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
            <ion-label>{{ $t("settings.appearance") }}</ion-label>
            <span slot="end" class="ft-choices ft-segment" role="group" :aria-label="$t('settings.appearance')">
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
          <ion-item button detail lines="none" data-test="move" @click="router.push('/move?role=old')">
            <span slot="start" class="ft-tile"><ion-icon :icon="swapHorizontalOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.movePhone") }}</ion-label>
          </ion-item>
          <!-- §78: the router forgets this device and the phone is wiped. It asks first. -->
          <ion-item v-if="!asksToErase" button lines="none" data-test="erase" @click="asksToErase = true">
            <span slot="start" class="ft-tile"><ion-icon :icon="trashOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ $t("settings.erase") }}
              <p class="ft-muted">{{ $t("settings.eraseHint") }}</p>
            </ion-label>
          </ion-item>
          <ion-item v-else lines="none" class="ft-erase">
            <span slot="start" class="ft-tile"><ion-icon :icon="trashOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.eraseAsk") }}</ion-label>
            <span slot="end" class="ft-choices">
              <button type="button" class="ft-erase__cancel" data-test="erase-cancel" @click="asksToErase = false">
                {{ $t("common.cancel") }}
              </button>
              <button type="button" class="ft-erase__go" data-test="erase-confirm" @click="erase">
                {{ $t("settings.eraseConfirm") }}
              </button>
            </span>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="sparklesOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.plan") }}</ion-label>
            <ion-note slot="end">{{ plan }}</ion-note>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="informationCircleOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.version") }}</ion-label>
            <ion-note slot="end">{{ version }}</ion-note>
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

.ft-erase__cancel,
.ft-erase__go {
  appearance: none;
  padding: 7px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 999px;
  background: transparent;
  color: var(--ft-text);
  font: inherit;
  font-size: 14px;
  cursor: pointer;
}
.ft-erase__go {
  border-color: transparent;
  background: var(--ion-color-danger);
  color: #fff;
  font-weight: 600;
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
