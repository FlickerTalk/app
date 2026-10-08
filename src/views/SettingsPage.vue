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
  onIonViewWillLeave,
  toastController,
} from "@ionic/vue";
import {
  appsOutline,
  banOutline,
  bulbOutline,
  callOutline,
  checkmarkDoneOutline,
  checkmarkOutline,
  colorPaletteOutline,
  constructOutline,
  contrastOutline,
  copyOutline,
  fileTrayOutline,
  informationCircleOutline,
  keypadOutline,
  lockClosedOutline,
  moonOutline,
  notificationsOffOutline,
  phonePortraitOutline,
  qrCodeOutline,
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
import { closeOnBackWhile } from "../back";
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
import { extraTab, setCallRouting, setExtraTab, storedCallRouting, type CallRouting, type ExtraTab } from "../preferences";
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

// Plan §40–§47 (Ioan, 2026-10-08): chat, calls, files and games are free forever. The premium part
// (the tools and the sessions with a PIN) is free for 15 days from the install, then a yearly
// subscription at the Store's price. It all lives in the Premium section here (mockup
// settings-premium.html); the Plan screen is gone. Everything is decided on this phone.
const planView = ref<PlanView | null>(null);
/** Whether the premium part is locked now: the free days are over and nothing is paid. */
const premiumLocked = computed(() => planView.value?.state === "limited");
/** What a year costs, as the Store formats it; null while unknown or when it cannot say. */
const price = ref<string | null>(null);
/** What the Store answered to a purchase or a restore, already in the user's words. */
const trouble = ref("");

/**
 * Asks the Store what a year costs (on entering, when the plan changes and back on the screen,
 * since the Store's account or country may have changed). The Store may take a while or not answer
 * at all: the screen does not wait for it, and keeps the last price it said.
 */
function askPrice() {
  void subscriptionPrice().then((said) => {
    if (said) price.value = said;
  });
}

/** The line under the heading: what is free and what is paid, or until when it is paid. */
const premiumNote = computed(() => {
  const plan = planView.value;
  switch (plan?.state) {
    case "trial":
      return price.value ? t("premium.trialNote", { price: price.value }) : t("premium.trialNoteYearly");
    case "limited":
      return price.value ? t("premium.limitedNote", { price: price.value }) : t("premium.limitedNoteYearly");
    case "subscribed":
      // 2026-10-07: Google Play never says until when (`renews`); StoreKit's date is a real expiry.
      return plan.renews || !plan.until
        ? t("premium.paid")
        : t("premium.paidUntil", { until: new Date(plan.until).toLocaleDateString() });
    default:
      return "";
  }
});

/** With the Store's price, or with no amount at all: the app never writes one of its own. */
const payText = computed(() => (price.value ? t("premium.subscribe", { price: price.value }) : t("premium.subscribeYearly")));

/** The badge on both premium rows: the days left of the trial, or that it is paid. None when locked. */
const premiumBadge = computed(() => {
  const plan = planView.value;
  if (plan?.state === "trial") return { text: t("premium.daysLeft", { days: daysLeft(plan.until) }), color: "primary", icon: null };
  if (plan?.state === "subscribed") return { text: t("premium.active"), color: "success", icon: checkmarkOutline };
  return null;
});

/** The two premium rows; locked, each says what it needs. */
const premiumRows = [
  { test: "plugins", label: "premium.tools", locked: "plugins.locked", icon: constructOutline, path: "/plugins" },
  // Hidden sessions: a PIN pad, nothing else. The same six digits create or enter one. §108: locked,
  // the row never reaches the pad, so nothing after a PIN tells whether a session exists.
  { test: "session", label: "premium.sessions", locked: "session.subscribeToUse", icon: keypadOutline, path: "/session" },
];

/** A premium row opens its page; locked, it asks the Store, as the Subscribe button above it does. */
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

/** A year bought on another phone, or before a reinstall, comes back from the Store (2026-10-07). */
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

// `/plan` and everything locked elsewhere (a tool, the PIN pad) come here at `#premium`. The
// section is scrolled to through Ionic's content area: `scrollIntoView` while the page is still
// entering does nothing (seen in Chromium, 2026-10-08).
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

// The ID on the card, to paste anywhere. The tick says so only once the clipboard took it, and for
// a moment: then the button copies again.
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
// Issue app#7: whether the weekly hours are on, shown on their row.
const hoursOn = ref(false);
onMounted(async () => {
  version.value = await getVersion().catch(() => "");
  hoursOn.value = (await quietHours().catch(() => null)) !== null;
});
// Settings stays alive behind the tabs, so the plan and the price are read again each time it
// comes back, and when the app does.
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
// And when the Store changes its mind with the app open (2026-10-07): a renewal, an expiry, an
// approved Ask to Buy. What the Store said before is no longer true then.
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
  { id: "lime", label: t("colors.lime"), swatch: "linear-gradient(135deg, #000000 50%, #c6f432 50%)" },
];
const appearances: { id: Appearance; label: string; icon: string }[] = [
  { id: "system", label: t("settings.system"), icon: phonePortraitOutline },
  { id: "dark", label: t("settings.dark"), icon: moonOutline },
  { id: "light", label: t("settings.light"), icon: sunnyOutline },
];

const direction = ref(storedDirection());
const asksToErase = ref(false);

// A4: up to what size a file comes on its own; 0 asks every time, the last choice never asks.
// The size keeps its number and unit together (a no-break space) when the row's value wraps.
const autoDownloadLabel = (bytes: number) =>
  bytes === 0
    ? t("settings.autoDownloadAsk")
    : bytes === Number.MAX_SAFE_INTEGER
      ? t("settings.autoDownloadAlways")
      : t("settings.autoDownloadUpTo", { size: formatSize(bytes).replace(" ", "\u00a0") });

async function onAutoDownloadChange(event: CustomEvent<{ value: number }>) {
  await setAutoDownload(Number(event.detail.value));
}

// A5: a new link retires the old one; it asks once, because whoever kept it is cut off.
const asksToRenew = ref(false);
const renewed = ref(false);

async function renew() {
  asksToRenew.value = false;
  await renewLink();
  renewed.value = true;
}

// §78: it takes this device off the router and wipes the phone, so it asks first. Then the app
// starts again at the welcome; until it does, the screen says the phone is being erased
// (2026-09-30), and says so if it could not be.
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

// 2026-10-02: an anonymous suggestion, in a modal over Settings (Ioan); Back closes it, and closing
// forgets what was written.
const suggesting = ref(false);
// Only while Settings is on screen: a reminder or a call can put a page over it with the modal open.
const onScreen = ref(true);
onIonViewWillLeave(() => (onScreen.value = false));
onIonViewDidEnter(() => (onScreen.value = true));
closeOnBackWhile(() => onScreen.value && suggesting.value, () => (suggesting.value = false));

function chooseColor(id: Direction) {
  direction.value = id;
  applyDirection(id);
}

function chooseAppearance(id: Appearance) {
  appearance.value = id;
  applyAppearance(id);
}

// 2026-10-05 (Ioan): the fourth tab is the games, the plugins or none (the default). The bar and
// the rail change at once, behind the sheet.
function onExtraTabChange(event: CustomEvent<{ value: ExtraTab }>) {
  setExtraTab(event.detail.value);
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
          <!-- My code is «Add contact» on its first tab: the QR another phone scans to add this one. -->
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
          <!-- Issue app#6: the default for new contacts; each contact's page can differ. -->
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
          <!-- A4: bigger files wait for a tap; the size is this phone's choice. -->
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
          <!-- A5: whoever has the old link is cut off; the contacts get the new card by themselves. -->
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
          <!-- Issue app#7: the weekly hours when the phone may make noise. -->
          <ion-item button detail lines="none" data-test="hours" @click="router.push('/hours')">
            <span slot="start" class="ft-tile"><ion-icon :icon="notificationsOffOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.hours") }}</ion-label>
            <ion-note v-if="!hoursOn" slot="end">{{ $t("settings.hoursOff") }}</ion-note>
          </ion-item>
          <!-- Plan §17/§67: "always" hides your IP from the contact; "direct" never uses our relay. -->
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
          <!-- 2026-10-05: what the bar shows between Calls and Settings: nothing, the games or the plugins. -->
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="appsOutline" aria-hidden="true" /></span>
            <ion-select
              :value="extraTab"
              data-test="extra-tab"
              :aria-label="$t('settings.tabBar')"
              interface="modal"
              :interface-options="{ header: $t('settings.tabBar') }"
              :cancel-text="$t('common.cancel')"
              @ion-change="onExtraTabChange"
            >
              <div slot="label">{{ $t("settings.tabBar") }}</div>
              <ion-select-option value="none">{{ $t("settings.tabBarNone") }}</ion-select-option>
              <ion-select-option value="games">{{ $t("tabs.games") }}</ion-select-option>
              <ion-select-option value="plugins">{{ $t("tabs.plugins") }}</ion-select-option>
            </ion-select>
          </ion-item>
        </ion-list>

        <!-- Premium (2026-10-08): the tools and the sessions with a PIN; the subscription lives here. -->
        <section ref="premiumSection" id="premium" class="ft-premium" data-test="premium">
          <h2 class="ft-premium__head" data-test="premium-head">
            <span class="ft-premium__rule" aria-hidden="true" />
            <span class="ft-premium__title"><ion-icon :icon="sparklesOutline" aria-hidden="true" />{{ $t("premium.title") }}</span>
            <span class="ft-premium__rule" aria-hidden="true" />
          </h2>
          <ion-text v-if="premiumNote" color="medium">
            <p class="ft-premium__note" data-test="premium-note">{{ premiumNote }}</p>
          </ion-text>
          <div v-if="premiumLocked" class="ft-premium__cta">
            <ion-button expand="block" data-test="pay" @click="pay">{{ payText }}</ion-button>
          </div>
          <ion-list inset class="ft-group">
            <!-- Issue app#3: what runs inside FlickerTalk, and what each one may do. -->
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
            <!-- A year bought on another phone or before a reinstall (App Review guideline 3.1.1). -->
            <ion-item v-if="premiumLocked" button :detail="false" lines="none" data-test="restore" @click="restore">
              <ion-label color="medium" class="ft-premium__restore">{{ $t("plan.restore") }}</ion-label>
            </ion-item>
          </ion-list>
          <ion-text v-if="trouble" color="danger">
            <p class="ft-premium__note" role="alert" data-test="trouble">{{ trouble }}</p>
          </ion-text>
        </section>

        <ion-list inset class="ft-group">
          <!-- Plan §60: a QR pairs both phones for a direct P2P transfer; it never contains the key. -->
          <ion-item button detail lines="none" data-test="move" @click="router.push('/move?role=old')">
            <span slot="start" class="ft-tile"><ion-icon :icon="swapHorizontalOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.movePhone") }}</ion-label>
          </ion-item>
          <!-- 2026-09-27: a sealed copy of this phone in the user's own cloud (§61). -->
          <ion-item button detail lines="none" data-test="backup" @click="router.push('/backup')">
            <span slot="start" class="ft-tile"><ion-icon :icon="cloudUploadOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("settings.backup") }}</ion-label>
          </ion-item>
          <!-- §78: the router forgets this device and the phone is wiped. It asks first. -->
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

        <ion-list inset class="ft-group">
          <!-- 2026-10-02: an anonymous suggestion, mailed on by the router, which keeps nothing. -->
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

/* A long note at the end of a row (the hours, a chosen value) wraps before the row's label gives up a
   word: without this Ionic shrinks the label's box to nothing (iOS) or breaks its word (Android). */
.ft-group ion-item::part(container) {
  min-width: min-content;
}

.ft-item__title {
  display: block;
}

/* Premium (mockup of 2026-10-08): a heading between two thin rules, a note, the rows. */
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
/* Spacing letters apart breaks joined and combining scripts. */
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
/* Ionic's badge, tinted: the colour's text on a faint wash of it, as the mockup draws it. */
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
