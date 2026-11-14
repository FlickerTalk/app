<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { getVersion } from "@tauri-apps/api/app";
import { useRoute, useRouter } from "vue-router";
import {
  IonBadge,
  IonButton,
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
  IonSpinner,
  IonText,
  IonTitle,
  IonToggle,
  IonToolbar,
  onIonViewDidEnter,
  onIonViewWillEnter,
  toastController,
} from "@ionic/vue";
import {
  banOutline,
  bulbOutline,
  callOutline,
  checkmarkDoneOutline,
  checkmarkOutline,
  colorPaletteOutline,
  constructOutline,
  contrastOutline,
  copyOutline,
  documentTextOutline,
  fileTrayOutline,
  helpCircleOutline,
  informationCircleOutline,
  keypadOutline,
  lockClosedOutline,
  mailOutline,
  moonOutline,
  notificationsOffOutline,
  openOutline,
  phonePortraitOutline,
  qrCodeOutline,
  shieldCheckmarkOutline,
  sparklesOutline,
  sunnyOutline,
  swapHorizontalOutline,
  trashOutline,
  cloudDownloadOutline,
  cloudUploadOutline,
  refreshOutline,
} from "ionicons/icons";
import Avatar from "../components/Avatar.vue";
import FeedbackModal from "../components/FeedbackModal.vue";
import QuickTour from "../components/QuickTour.vue";
import { closeOnBackWhile } from "../back";
import { useOnScreen } from "../on-screen";
import {
  AUTO_DOWNLOAD_CHOICES,
  autoDownloadChoice,
  daysLeft,
  erasePhone,
  followPlan,
  formatSize,
  payTrouble,
  plan as planOf,
  quietHours,
  renewLink,
  restoreSubscription,
  setAutoDownload,
  setMailbox,
  setReceipts,
  store,
  subscribe,
  subscriptionPrice,
  type PlanView,
} from "../core";
import { PRIVACY_URL, SUPPORT_ADDRESS, TERMS_URL, mailSupport, openOutside } from "../legal";
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
const route = useRoute();

const planView = ref<PlanView | null>(null);
const premiumLocked = computed(() => planView.value?.state === "limited");
const offersPurchase = computed(() => planView.value?.state === "trial" || premiumLocked.value);
const price = ref<string | null>(null);
const trouble = ref("");

// the Store may be slow or never answer, so don't wait and keep the last price
function askPrice() {
  void subscriptionPrice().then((said) => {
    if (said) price.value = said;
  });
}

const premiumNote = computed(() => {
  const plan = planView.value;
  switch (plan?.state) {
    case "trial":
      return price.value ? t("premium.trialNote", { price: price.value }) : t("premium.trialNoteYearly");
    case "limited":
      return price.value ? t("premium.limitedNote", { price: price.value }) : t("premium.limitedNoteYearly");
    case "subscribed":
      // Google Play never says until when (renews), StoreKit's date is a real expiry
      return plan.renews || !plan.until
        ? t("premium.paid")
        : t("premium.paidUntil", { until: new Date(plan.until).toLocaleDateString() });
    default:
      return "";
  }
});

// only the Store's price, the app never writes an amount
const payText = computed(() => (price.value ? t("premium.subscribe", { price: price.value }) : t("premium.subscribeYearly")));

const premiumBadge = computed(() => {
  const plan = planView.value;
  if (plan?.state === "trial") return { text: t("premium.daysLeft", { days: daysLeft(plan.until) }), color: "primary", icon: null };
  if (plan?.state === "subscribed") return { text: t("premium.active"), color: "success", icon: checkmarkOutline };
  return null;
});

const premiumRows = [
  { test: "plugins", label: "premium.tools", locked: "plugins.locked", icon: constructOutline, path: "/tabs/apps?show=tools" },
  // locked, the row never reaches the PIN pad, so no PIN can tell whether a session exists
  { test: "session", label: "premium.sessions", locked: "session.subscribeToUse", icon: keypadOutline, path: "/session" },
];

function openPremium(path: string) {
  if (premiumLocked.value) void pay();
  else void router.push(path);
}

async function pay() {
  trouble.value = "";
  try {
    await subscribe();
  } catch (error) {
    const say = payTrouble(error);
    trouble.value = say ? t(say) : "";
  }
  await refreshPlan();
}

async function restore() {
  trouble.value = "";
  try {
    const found = await restoreSubscription();
    const notice = await toastController.create({
      message: t(found === "restored" ? "plan.restored" : "plan.nothingToRestore"),
      duration: 2500,
      position: "bottom",
    });
    await notice.present();
  } catch (error) {
    const say = payTrouble(error);
    trouble.value = say ? t(say) : "";
  }
  await refreshPlan();
}

// scrollIntoView does nothing while the page is still entering, so scroll Ionic's content
type ScrollArea = HTMLElement & {
  componentOnReady?: () => Promise<unknown>;
  getScrollElement?: () => Promise<HTMLElement>;
  scrollToPoint?: (x: number, y: number, ms: number) => Promise<void>;
};
const content = ref<{ $el: ScrollArea } | null>(null);
const premiumSection = ref<HTMLElement | null>(null);
async function showPremiumIfAsked() {
  if (route.hash !== "#premium") return;
  const area = content.value?.$el;
  const section = premiumSection.value;
  if (!area?.getScrollElement || !area.scrollToPoint || !section) return;
  await area.componentOnReady?.();
  const scroller = await area.getScrollElement();
  const y = scroller.scrollTop + section.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
  await area.scrollToPoint(0, Math.max(0, y), 300);
}
onMounted(showPremiumIfAsked);
onIonViewDidEnter(showPremiumIfAsked);
watch(() => route.fullPath, showPremiumIfAsked);

const idCopied = ref(false);
let idCopiedTimer: ReturnType<typeof setTimeout> | undefined;
async function copyId() {
  try {
    await navigator.clipboard.writeText(store.me.id);
  } catch {
    return;
  }
  idCopied.value = true;
  clearTimeout(idCopiedTimer);
  idCopiedTimer = setTimeout(() => (idCopied.value = false), 2000);
}

const version = ref("");
const hoursOn = ref<boolean | null>(null);
async function readHours() {
  hoursOn.value = (await quietHours().catch(() => null)) !== null;
}
onMounted(async () => {
  version.value = await getVersion().catch(() => "");
  await readHours();
});
onIonViewWillEnter(() => void readHours());
// Ionic doesn't always fire that hook on a tab page (Calls → Settings → Hours → back), so follow the
// route too
const SETTINGS_PATH = "/tabs/settings";
watch(
  () => route.path === SETTINGS_PATH,
  (here) => {
    if (here) void readHours();
  },
);
async function refreshPlan() {
  planView.value = await planOf().catch(() => null);
}
function onVisible() {
  if (document.visibilityState !== "visible") return;
  void refreshPlan();
  askPrice();
}
onMounted(() => {
  void refreshPlan();
  askPrice();
  document.addEventListener("visibilitychange", onVisible);
});
onIonViewWillEnter(() => {
  void refreshPlan();
  askPrice();
});
let stopFollowingPlan: (() => void) | null = null;
let gone = false;
onMounted(() =>
  void followPlan(() => {
    trouble.value = "";
    void refreshPlan();
    askPrice();
  }).then((stop) => (gone ? stop() : (stopFollowingPlan = stop))),
);
onBeforeUnmount(() => {
  gone = true;
  document.removeEventListener("visibilitychange", onVisible);
  stopFollowingPlan?.();
});

async function onReceiptsChange(event: CustomEvent<{ checked: boolean }>) {
  await setReceipts(event.detail.checked);
}


// the server never stores this, the core tells contacts
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
  { id: "lime", label: t("colors.lime"), swatch: "linear-gradient(135deg, #000000 50%, #c6f432 50%)" },
];
const appearances: { id: Appearance; label: string; icon: string }[] = [
  { id: "system", label: t("settings.system"), icon: phonePortraitOutline },
  { id: "dark", label: t("settings.dark"), icon: moonOutline },
  { id: "light", label: t("settings.light"), icon: sunnyOutline },
];

const direction = ref(storedDirection());
const asksToErase = ref(false);

// 0 always asks. A no-break space keeps number and unit together.
const autoDownloadLabel = (bytes: number) =>
  bytes === 0
    ? t("settings.autoDownloadAsk")
    : bytes === Number.MAX_SAFE_INTEGER
      ? t("settings.autoDownloadAlways")
      : t("settings.autoDownloadUpTo", { size: formatSize(bytes).replace(" ", "\u00a0") });

async function onAutoDownloadChange(event: CustomEvent<{ value: number }>) {
  await setAutoDownload(Number(event.detail.value));
}

const asksToRenew = ref(false);
const renewed = ref(false);

async function renew() {
  asksToRenew.value = false;
  await renewLink();
  renewed.value = true;
}

const erasing = ref(false);
const eraseFailed = ref(false);

async function erase() {
  asksToErase.value = false;
  eraseFailed.value = false;
  erasing.value = true;
  try {
    await erasePhone();
  } catch {
    erasing.value = false;
    eraseFailed.value = true;
  }
}
const appearance = ref(storedAppearance());

const suggesting = ref(false);
// a reminder or a call can put a page over Settings with the modal open
const onScreen = useOnScreen();
closeOnBackWhile(() => onScreen.value && suggesting.value, () => (suggesting.value = false));

const touring = ref(false);

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
        <ion-title><span class="ft-title">{{ $t("settings.title") }}</span></ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content ref="content">
      <ion-header collapse="condense" class="ion-no-border">
        <ion-toolbar>
          <ion-title size="large"><span class="ft-title">{{ $t("settings.title") }}</span></ion-title>
        </ion-toolbar>
      </ion-header>

      <div class="ft-settings">
        <section class="ft-me">
          <Avatar :name="store.me.name || store.me.id" :hue="store.me.hue" :size="60" />
          <span class="ft-me__text">
            <span class="ft-me__label">{{ $t("settings.yourId") }}</span>
            <code class="ft-me__id">{{ store.me.id }}</code>
          </span>
          <button
            type="button"
            class="ft-round ft-round--ghost"
            data-test="copy-id"
            :aria-label="idCopied ? $t('settings.idCopied') : $t('settings.copyId')"
            @click="copyId"
          >
            <ion-icon :icon="idCopied ? checkmarkOutline : copyOutline" aria-hidden="true" />
          </button>
          <button
            type="button"
            class="ft-round ft-round--accent"
            data-test="show-qr"
            :aria-label="$t('settings.showQr')"
            @click="router.push('/add-contact')"
          >
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
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="checkmarkDoneOutline" aria-hidden="true" /></span>
            <ion-toggle
              :checked="store.me.receipts"
              data-test="receipts"
              :aria-label="$t('settings.receipts')"
              @ion-change="onReceiptsChange"
            >
              <span class="ft-item__title">{{ $t("settings.receipts") }}</span>
              <span class="ft-item__note">{{ $t("settings.receiptsNote") }}</span>
            </ion-toggle>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="cloudDownloadOutline" aria-hidden="true" /></span>
            <ion-select
              :value="autoDownloadChoice(store.me.autoDownload)"
              data-test="auto-download"
              :aria-label="$t('settings.autoDownload')"
              interface="action-sheet"
              :cancel-text="$t('common.cancel')"
              @ion-change="onAutoDownloadChange"
            >
              <div slot="label">{{ $t("settings.autoDownload") }}</div>
              <ion-select-option v-for="bytes in AUTO_DOWNLOAD_CHOICES" :key="bytes" :value="bytes">{{ autoDownloadLabel(bytes) }}</ion-select-option>
            </ion-select>
          </ion-item>
          <ion-item v-if="!asksToRenew" button lines="none" data-test="renew-link" @click="asksToRenew = true">
            <span slot="start" class="ft-tile"><ion-icon :icon="refreshOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ $t("settings.renewLink") }}
              <p class="ft-muted">{{ renewed ? $t("settings.renewed") : $t("settings.renewLinkHint") }}</p>
            </ion-label>
          </ion-item>
          <ion-item v-else lines="none" class="ft-erase">
            <span slot="start" class="ft-tile"><ion-icon :icon="refreshOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.renewLinkHint") }}</ion-label>
            <span slot="end" class="ft-choices">
              <button type="button" class="ft-erase__cancel" data-test="renew-cancel" @click="asksToRenew = false">
                {{ $t("common.cancel") }}
              </button>
              <button type="button" class="ft-erase__go" data-test="renew-confirm" @click="renew">
                {{ $t("settings.renewLinkConfirm") }}
              </button>
            </span>
          </ion-item>
          <ion-item button detail lines="none" data-test="hours" @click="router.push('/hours')">
            <span slot="start" class="ft-tile"><ion-icon :icon="notificationsOffOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.hours") }}</ion-label>
            <ion-note v-if="hoursOn !== null" slot="end">{{ $t(hoursOn ? "settings.hoursOn" : "settings.hoursOff") }}</ion-note>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="callOutline" aria-hidden="true" /></span>
            <ion-select
              :value="callRouting"
              :aria-label="$t('settings.calls')"
              interface="action-sheet"
              :cancel-text="$t('common.cancel')"
              @ion-change="onCallRoutingChange"
            >
              <div slot="label">{{ $t("settings.calls") }}</div>
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

        <section ref="premiumSection" id="premium" class="ft-premium" data-test="premium">
          <h2 class="ft-premium__head" data-test="premium-head">
            <span class="ft-premium__rule" aria-hidden="true" />
            <span class="ft-premium__title"><ion-icon :icon="sparklesOutline" aria-hidden="true" />{{ $t("premium.title") }}</span>
            <span class="ft-premium__rule" aria-hidden="true" />
          </h2>
          <ion-text v-if="premiumNote" color="medium">
            <p class="ft-premium__note" data-test="premium-note">{{ premiumNote }}</p>
          </ion-text>
          <div v-if="offersPurchase" class="ft-premium__cta">
            <ion-button expand="block" data-test="pay" @click="pay">{{ payText }}</ion-button>
          </div>
          <ion-list inset class="ft-group">
            <ion-item
              v-for="row in premiumRows"
              :key="row.test"
              button
              :detail="!premiumLocked"
              lines="none"
              :class="{ 'ft-premium__locked': premiumLocked }"
              :data-test="row.test"
              :aria-label="premiumLocked ? $t(row.locked) : undefined"
              @click="openPremium(row.path)"
            >
              <span slot="start" class="ft-tile"><ion-icon :icon="row.icon" aria-hidden="true" /></span>
              <ion-label>{{ $t(row.label) }}</ion-label>
              <ion-badge v-if="premiumBadge" slot="end" :color="premiumBadge.color" class="ft-premium__badge" data-test="premium-badge">
                <ion-icon v-if="premiumBadge.icon" :icon="premiumBadge.icon" aria-hidden="true" />{{ premiumBadge.text }}
              </ion-badge>
              <ion-icon v-if="premiumLocked" slot="end" :icon="lockClosedOutline" color="medium" data-test="premium-locked" aria-hidden="true" />
            </ion-item>
            <!-- App Review 3.1.1 -->
            <ion-item v-if="offersPurchase" button :detail="false" lines="none" data-test="restore" @click="restore">
              <ion-label color="medium" class="ft-premium__restore">{{ $t("plan.restore") }}</ion-label>
            </ion-item>
          </ion-list>
          <ion-text v-if="trouble" color="danger">
            <p class="ft-premium__note" role="alert" data-test="trouble">{{ trouble }}</p>
          </ion-text>
        </section>

        <ion-list inset class="ft-group">
          <ion-item button detail lines="none" data-test="move" @click="router.push('/move?role=old')">
            <span slot="start" class="ft-tile"><ion-icon :icon="swapHorizontalOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.movePhone") }}</ion-label>
          </ion-item>
          <ion-item button detail lines="none" data-test="backup" @click="router.push('/backup')">
            <span slot="start" class="ft-tile"><ion-icon :icon="cloudUploadOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.backup") }}</ion-label>
          </ion-item>
          <ion-item v-if="erasing" lines="none" data-test="erasing" role="status">
            <span slot="start" class="ft-tile"><ion-spinner name="crescent" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.erasing") }}</ion-label>
          </ion-item>
          <ion-item v-else-if="!asksToErase" button lines="none" data-test="erase" @click="asksToErase = true">
            <span slot="start" class="ft-tile"><ion-icon :icon="trashOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ $t("settings.erase") }}
              <p class="ft-muted">{{ $t("settings.eraseHint") }}</p>
              <p v-if="eraseFailed" class="ft-erase__failed" role="alert">{{ $t("settings.eraseFailed") }}</p>
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

        <!-- App Review 1.2 wants these reachable from the app -->
        <ion-list inset class="ft-group">
          <ion-item button detail lines="none" data-test="tour" @click="touring = true">
            <span slot="start" class="ft-tile"><ion-icon :icon="helpCircleOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("tour.title") }}</ion-label>
          </ion-item>
          <ion-item button detail :detail-icon="openOutline" lines="none" data-test="terms" @click="openOutside(TERMS_URL)">
            <span slot="start" class="ft-tile"><ion-icon :icon="documentTextOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.terms") }}</ion-label>
          </ion-item>
          <ion-item
            button
            detail
            :detail-icon="openOutline"
            lines="none"
            data-test="privacy-policy"
            @click="openOutside(PRIVACY_URL)"
          >
            <span slot="start" class="ft-tile"><ion-icon :icon="shieldCheckmarkOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.privacyPolicy") }}</ion-label>
          </ion-item>
          <ion-item button detail :detail-icon="openOutline" lines="none" data-test="contact" @click="mailSupport">
            <span slot="start" class="ft-tile"><ion-icon :icon="mailOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ $t("settings.contact") }}
              <p>{{ SUPPORT_ADDRESS }}</p>
            </ion-label>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item button detail lines="none" data-test="feedback" @click="suggesting = true">
            <span slot="start" class="ft-tile"><ion-icon :icon="bulbOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.feedback") }}</ion-label>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="informationCircleOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.version") }}</ion-label>
            <ion-note slot="end">{{ version }}</ion-note>
          </ion-item>
        </ion-list>
      </div>
    </ion-content>

    <FeedbackModal :open="suggesting" @close="suggesting = false" />
    <QuickTour class="ft-sheet--tab" :open="touring && onScreen" @close="touring = false" />
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

.ft-erase__failed {
  color: var(--ion-color-danger);
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

/* without this Ionic shrinks the label's box to nothing (iOS) or breaks its word (Android) */
.ft-group ion-item::part(container) {
  min-width: min-content;
}

.ft-item__title {
  display: block;
}

.ft-premium {
  margin-top: var(--ft-space-5);
}
.ft-premium__head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0 20px;
  color: var(--ion-color-medium);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}
/* letter spacing breaks joined scripts */
.ft-premium__head:lang(ar),
.ft-premium__head:lang(hi),
.ft-premium__head:lang(bn),
.ft-premium__head:lang(th) {
  letter-spacing: normal;
}
.ft-premium__rule {
  flex: 1;
  height: 1px;
  background: currentColor;
  opacity: 0.35;
}
.ft-premium__title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.ft-premium__note {
  margin: 8px 20px 0;
  font-size: 13px;
  line-height: 1.4;
}
.ft-premium__cta {
  margin: 10px 16px 0;
}
.ft-premium__cta ion-button {
  margin: 0;
  font-weight: 600;
}
.ft-premium__badge {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 8px;
  border-radius: 999px;
  background: rgba(var(--ion-color-base-rgb), 0.16);
  color: var(--ion-color-base);
  font-size: 12px;
  font-weight: 500;
}
.ft-premium__locked ion-label {
  color: var(--ion-color-medium);
}
.ft-premium__locked .ft-tile {
  background: rgba(var(--ion-color-medium-rgb), 0.14);
  color: var(--ion-color-medium);
}
.ft-premium__restore {
  text-align: center;
  font-size: 14px;
}
.ft-item__note {
  display: block;
  font-size: 12px;
  color: var(--ft-muted);
}
</style>
