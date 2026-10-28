<script lang="ts">
/** Tours made so far: each one's steps get ids no other tour on the screen has. */
let tours = 0;
</script>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import {
  IonButton,
  IonButtons,
  IonContent,
  IonFooter,
  IonHeader,
  IonIcon,
  IonModal,
  IonSegment,
  IonSegmentButton,
  IonSegmentContent,
  IonSegmentView,
  IonToolbar,
} from "@ionic/vue";
import {
  appsOutline,
  callOutline,
  chatbubblesOutline,
  ellipse,
  flagOutline,
  helpBuoyOutline,
  lockClosedOutline,
  qrCodeOutline,
  swapHorizontalOutline,
} from "ionicons/icons";
import { closeOnBackWhile } from "../back";
import { t } from "../i18n";
import { SUPPORT_ADDRESS } from "../legal";
import { setTourSeen } from "../preferences";

// The quick tour (2026-10-28, Ioan: "the typical quick tour"). A new identity sees it once, when the
// chats open after Start; Settings and the empty chat list open it again whenever asked. Ionic's
// sheet modal, as tall as the screen, with eight steps in Ionic's swipeable segment view (the
// segment under it is the row of dots, and it swipes the other way in Arabic on its own). Skip is
// always there and closing is for good: Skip, Done, a swipe down, a tap outside or Back. It only
// says what the app really does (brand rules: short, sober, no "forever").
const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: [] }>();

const STEPS = [
  { id: "contact", icon: qrCodeOutline },
  { id: "messages", icon: chatbubblesOutline },
  { id: "calls", icon: callOutline },
  { id: "apps", icon: appsOutline },
  { id: "safety", icon: flagOutline },
  { id: "privacy", icon: lockClosedOutline },
  { id: "support", icon: helpBuoyOutline },
  { id: "move", icon: swapHorizontalOutline },
] as const;
type Step = (typeof STEPS)[number]["id"];

/** What the steps name, as the app's own screens say it. */
const named = computed(() => ({
  requests: t("requests.title"),
  apps: t("tabs.apps"),
  address: SUPPORT_ADDRESS,
  settings: t("tabs.settings"),
  move: t("settings.movePhone"),
}));

const prefix = `ft-tour-${++tours}`;
const contentId = (step: Step) => `${prefix}-${step}`;

const current = ref<Step>(STEPS[0].id);
const at = computed(() => STEPS.findIndex((step) => step.id === current.value));
const last = computed(() => at.value === STEPS.length - 1);

// Each time it opens, from the first step.
watch(
  () => props.open,
  (open) => {
    if (open) current.value = STEPS[0].id;
  },
);

/** A swipe or a tap on a dot: Ionic's segment says which step is shown now. */
function shown(event: CustomEvent<{ value?: string | number }>) {
  const step = STEPS.find((one) => one.id === event.detail.value);
  if (step) current.value = step.id;
}

function next() {
  if (last.value) finish();
  else current.value = STEPS[at.value + 1].id;
}

/** Skipped, done or dismissed: it does not show on its own again. */
function finish() {
  setTourSeen();
  emit("close");
}

/**
 * Ionic dismissed it. While it is still meant to be open, Ionic did so on its own (a swipe down, a
 * tap outside), which is the user closing it; after its page closed it, there is nothing to say.
 */
function dismissed() {
  if (props.open) finish();
}

// Back is the user closing it. Everything on top closing for a link's page is not: the tour goes,
// still owed, and comes back with its page.
closeOnBackWhile(
  () => props.open,
  (why) => (why === "back" ? finish() : emit("close")),
);
</script>

<template>
  <ion-modal
    :is-open="open"
    class="ft-tour"
    :breakpoints="[0, 1]"
    :initial-breakpoint="1"
    :aria-label="$t('tour.title')"
    data-test="quick-tour"
    @did-dismiss="dismissed"
  >
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="end">
          <ion-button data-test="tour-skip" @click="finish">{{ $t("tour.skip") }}</ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>
    <ion-content :scroll-y="false">
      <ion-segment-view class="ft-tour__steps">
        <ion-segment-content v-for="step in STEPS" :id="contentId(step.id)" :key="step.id" data-test="tour-step">
          <div class="ft-tour__step">
            <ion-icon :icon="step.icon" color="primary" class="ft-tour__icon" aria-hidden="true" />
            <h2>{{ $t(`tour.steps.${step.id}.title`) }}</h2>
            <p>{{ $t(`tour.steps.${step.id}.body`, named) }}</p>
          </div>
        </ion-segment-content>
      </ion-segment-view>
    </ion-content>
    <ion-footer class="ion-no-border">
      <ion-toolbar>
        <ion-segment :value="current" class="ft-tour__dots" @ion-change="shown">
          <ion-segment-button
            v-for="(step, index) in STEPS"
            :key="step.id"
            :value="step.id"
            :content-id="contentId(step.id)"
            :aria-label="$t('tour.step', { step: index + 1, total: STEPS.length })"
          >
            <ion-icon :icon="ellipse" aria-hidden="true" />
          </ion-segment-button>
        </ion-segment>
        <ion-buttons slot="end">
          <ion-button fill="solid" shape="round" data-test="tour-next" @click="next">
            {{ last ? $t("tour.done") : $t("tour.next") }}
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-footer>
  </ion-modal>
</template>

<style scoped>
/* The steps fill the sheet; each one is centred in it, clear of a phone's side insets held sideways
   (the same room on both sides, so it does not matter which side the island is on). */
.ft-tour__steps {
  height: 100%;
}
.ft-tour__step {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: var(--ft-space-3);
  box-sizing: border-box;
  min-height: 100%;
  max-width: 34rem;
  margin-inline: auto;
  padding-block: var(--ft-space-5);
  padding-inline: max(var(--ft-space-5), var(--ion-safe-area-left, 0px), var(--ion-safe-area-right, 0px));
  text-align: center;
}
.ft-tour__icon {
  font-size: 72px;
}
.ft-tour__step h2 {
  margin: 0;
  font-size: 22px;
  font-weight: 700;
}
.ft-tour__step p {
  margin: 0;
  max-width: 32ch;
  color: var(--ion-color-medium);
  line-height: 1.45;
}
/* Ionic's segment as a row of dots: the step shown in the app's colour, the others muted, and no
   bar or pill of its own. Ionic spreads its buttons over the whole bar (a grid of equal columns);
   the dots keep together at the inline start, each a finger-tall target, on a phone or a tablet. */
.ft-tour__dots {
  width: fit-content;
  grid-auto-columns: 28px;
  --background: transparent;
}
.ft-tour__dots ion-segment-button {
  min-width: 0;
  min-height: 44px;
  --padding-start: 0;
  --padding-end: 0;
  --color: var(--ion-color-medium);
  --color-checked: var(--ion-color-primary);
  --background-checked: transparent;
  --indicator-color: transparent;
  --indicator-box-shadow: none;
  --border-width: 0;
}
.ft-tour__dots ion-segment-button ion-icon {
  font-size: 9px;
}
/* The app's buttons say things as written, not in Material's capitals. */
.ft-tour ion-button {
  text-transform: none;
}
</style>
