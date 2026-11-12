<script lang="ts">
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

// Ionic's segment view already swipes the other way in Arabic
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

watch(
  () => props.open,
  (open) => {
    if (open) current.value = STEPS[0].id;
  },
);

function shown(event: CustomEvent<{ value?: string | number }>) {
  const step = STEPS.find((one) => one.id === event.detail.value);
  if (step) current.value = step.id;
}

function next() {
  if (last.value) finish();
  else current.value = STEPS[at.value + 1].id;
}

function finish() {
  setTourSeen();
  emit("close");
}

// Ionic dismissing it on its own (swipe down, tap outside) is the user closing it
function dismissed() {
  if (props.open) finish();
}

// closing everything for a link's page doesn't count as seen, the tour comes back with its page
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
/* Ionic spreads segment buttons over equal columns, keep the dots together */
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
.ft-tour ion-button {
  text-transform: none;
}
</style>
