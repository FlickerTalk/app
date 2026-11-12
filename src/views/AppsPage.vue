<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  IonContent,
  IonHeader,
  IonIcon,
  IonLabel,
  IonPage,
  IonSegment,
  IonSegmentButton,
  IonTitle,
  IonToast,
  IonToolbar,
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { constructOutline, gameControllerOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import AppSheet from "../components/AppSheet.vue";
import AppTile from "../components/AppTile.vue";
import ContactPicker from "../components/ContactPicker.vue";
import GamePermissions from "../components/GamePermissions.vue";
import {
  formatSize,
  grantPlugin,
  installPlugin,
  needsSubscription,
  PREMIUM_PAGE,
  removePlugin,
  type OfferedPlugin,
  type PluginView,
} from "../core";
import { offerExactAlarms } from "../exact-alarms";
import { gameGrant, isGame, needsGameGrant } from "../games";
import { permissionsOf, withPermission } from "../permissions";
import {
  byPluginName,
  games,
  installed,
  isLocked,
  offered,
  pluginIcon,
  pluginImage,
  pluginName,
  refreshOffered,
  refreshPlugins,
  refreshPremiumLock,
  tools,
} from "../plugins";
import { closeOnBackWhile } from "../back";
import { useOnScreen } from "../on-screen";

const router = useRouter();
const route = useRoute();

type Segment = "tools" | "games";
const segment = ref<Segment>(route.query?.show === "games" ? "games" : "tools");
// the tab may already be mounted on the other segment
watch(
  () => route.query?.show,
  (show) => {
    if (show === "games" || show === "tools") segment.value = show;
  },
);
const SEGMENTS = [
  { id: "tools", icon: constructOutline, label: "apps.tools" },
  { id: "games", icon: gameControllerOutline, label: "apps.games" },
] as const;

const onGames = computed(() => segment.value === "games");
const mine = computed(() => (onGames.value ? games.value : tools.value));
const ofKind = computed(() => offered.value.filter((one) => isGame(one) === onGames.value));
const more = computed(() =>
  byPluginName(ofKind.value.filter((one) => !one.installed && !installed.value.some((here) => here.id === one.id))),
);

const pluginsRead = ref(false);
const offeredRead = ref(false);

const installing = ref("");
const failed = ref(false);

const sheetId = ref("");
const sheetOpen = ref(false);
const sheetPlugin = computed<PluginView | OfferedPlugin | null>(
  () => installed.value.find((one) => one.id === sheetId.value) ?? offered.value.find((one) => one.id === sheetId.value) ?? null,
);

const asking = ref<PluginView | null>(null);
const picking = ref("");

const onScreen = useOnScreen();
onIonViewWillLeave(() => {
  sheetOpen.value = false;
  asking.value = null;
  picking.value = "";
});
closeOnBackWhile(() => onScreen.value && sheetOpen.value, () => (sheetOpen.value = false));
closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => (asking.value = null));
closeOnBackWhile(() => onScreen.value && Boolean(picking.value), () => (picking.value = ""));

onMounted(refresh);
onIonViewWillEnter(refresh);

async function refresh() {
  await Promise.all([refreshPlugins(), refreshPremiumLock()]);
  pluginsRead.value = true;
  // offline the core answers with what the app carries
  await refreshOffered().catch(() => undefined);
  offeredRead.value = true;
}

function subscribe() {
  void router.push(PREMIUM_PAGE);
}

function badgeOf(one: PluginView | OfferedPlugin): "download" | "add" | "lock" | undefined {
  if (isLocked(one)) return "lock";
  if ("asks" in one) return undefined;
  return one.carried ? "add" : "download";
}

function weight(one: OfferedPlugin): string {
  return one.carried ? "" : formatSize(one.size);
}

function use(one: PluginView) {
  if (isGame(one)) return playGame(one);
  if (isLocked(one)) return subscribe();
  void router.push(`/plugin/${one.id}`);
}

async function install(one: OfferedPlugin) {
  if (isLocked(one)) return subscribe();
  if (installing.value) return;
  installing.value = one.id;
  failed.value = false;
  try {
    await installPlugin(one.id);
  } catch (error) {
    if (needsSubscription(error)) subscribe();
    else failed.value = true;
  } finally {
    installing.value = "";
  }
  await refresh();
}

function hold(one: PluginView | OfferedPlugin) {
  sheetId.value = one.id;
  sheetOpen.value = true;
}

function fromSheet(action: "open" | "play" | "install") {
  const one = sheetPlugin.value;
  sheetOpen.value = false;
  if (!one) return;
  if (action === "install") void install(one as OfferedPlugin);
  else use(one as PluginView);
}

async function removeFromSheet() {
  const id = sheetId.value;
  sheetOpen.value = false;
  if (!id) return;
  await removePlugin(id).catch(() => undefined);
  await refresh();
}

// two quick switches lost the first, the second grant was built from the stale plugin. One at a time,
// each from what the core says now.
let changes: Promise<void> = Promise.resolve();
function toggle(key: string, event: CustomEvent<{ checked: boolean }>): Promise<void> {
  const id = sheetId.value;
  const on = event.detail.checked;
  const field = event.target as { checked?: boolean } | null;
  changes = changes.then(async () => {
    const current = (await refreshPlugins()).find((one) => one.id === id);
    if (current) await grantPlugin(id, withPermission(current, key, on)).catch(() => undefined);
    const now = (await refreshPlugins()).find((one) => one.id === id);
    const real = now && permissionsOf(now).find((permission) => permission.key === key)?.on;
    if (field && real !== undefined) field.checked = real;
    if (key === "remind" && on && real) await offerExactAlarms();
  });
  return changes;
}

function playGame(game: PluginView) {
  if (needsGameGrant(game)) asking.value = game;
  else picking.value = game.id;
}

async function allow() {
  const game = asking.value;
  asking.value = null;
  if (!game) return;
  try {
    await grantPlugin(game.id, gameGrant(game));
  } catch {
    return;
  }
  await refreshPlugins();
  picking.value = game.id;
}

function addContact() {
  picking.value = "";
  void router.push("/add-contact");
}

function playWith(contact: string) {
  const game = picking.value;
  picking.value = "";
  void router.push(`/chat/${contact}?play=${game}`);
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-title><span class="ft-title">{{ $t("tabs.apps") }}</span></ion-title>
      </ion-toolbar>
      <ion-toolbar class="ft-apps__segment">
        <ion-segment :value="segment" @ion-change="segment = $event.detail.value === 'games' ? 'games' : 'tools'">
          <ion-segment-button v-for="one in SEGMENTS" :key="one.id" :value="one.id" layout="icon-start" :data-test="`apps-segment-${one.id}`">
            <ion-icon :icon="one.icon" aria-hidden="true" />
            <ion-label>{{ $t(one.label) }}</ion-label>
          </ion-segment-button>
        </ion-segment>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ft-apps">
      <div v-if="mine.length" class="ft-app-grid" data-test="apps-installed">
        <AppTile
          v-for="one in mine"
          :key="one.id"
          :name="pluginName(one)"
          :icon="pluginIcon(one)"
          :image="pluginImage(one)"
          :badge="badgeOf(one)"
          :data-test="`app-${one.id}`"
          @tap="use(one)"
          @hold="hold(one)"
        />
      </div>
      <p v-else-if="pluginsRead" class="ft-apps__note" data-test="apps-none">
        {{ onGames ? $t("apps.noGames") : $t("apps.noTools") }}
      </p>

      <template v-if="offeredRead || more.length">
        <div class="ft-apps__head">
          <span class="ft-apps__rule" aria-hidden="true" />
          <h2 data-test="apps-more-title">{{ onGames ? $t("apps.moreGames") : $t("apps.moreTools") }}</h2>
          <span class="ft-apps__rule" aria-hidden="true" />
        </div>
        <div v-if="more.length" class="ft-app-grid" data-test="apps-more">
          <AppTile
            v-for="one in more"
            :key="one.id"
            off
            :name="pluginName(one)"
            :icon="pluginIcon(one)"
            :image="pluginImage(one)"
            :badge="badgeOf(one)"
            :caption="weight(one)"
            :busy="installing === one.id"
            :data-test="`install-${one.id}`"
            @tap="install(one)"
            @hold="hold(one)"
          />
        </div>
        <p v-else-if="ofKind.length" class="ft-apps__note" data-test="apps-all-here">
          {{ onGames ? $t("apps.allGamesHere") : $t("apps.allToolsHere") }}
        </p>
        <p v-else class="ft-apps__note" data-test="apps-offline">{{ $t("apps.offline") }}</p>
      </template>
    </ion-content>

    <ion-toast
      :is-open="failed"
      :message="$t('apps.installFailed')"
      color="danger"
      :duration="4000"
      position="bottom"
      @did-dismiss="failed = false"
    />

    <AppSheet
      :plugin="sheetPlugin"
      :open="sheetOpen && Boolean(sheetPlugin)"
      :busy="Boolean(installing) && installing === sheetId"
      @open="fromSheet('open')"
      @play="fromSheet('play')"
      @install="fromSheet('install')"
      @remove="removeFromSheet"
      @toggle="toggle"
      @dismiss="sheetOpen = false"
    />

    <GamePermissions
      class="ft-sheet--tab"
      :open="Boolean(asking)"
      :name="asking ? pluginName(asking) : ''"
      :icon="asking ? pluginIcon(asking) : undefined"
      :image="asking ? pluginImage(asking) : undefined"
      @allow="allow"
      @cancel="asking = null"
    />

    <ContactPicker
      class="ft-apps__picker ft-sheet--tab"
      :open="Boolean(picking)"
      purpose="play"
      @pick="playWith"
      @add="addContact"
      @dismiss="picking = ''"
    />
  </ion-page>
</template>

<style scoped>
.ft-apps {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
.ft-apps__segment ion-segment-button {
  min-height: 44px;
  text-transform: none;
  letter-spacing: 0;
  font-size: 14px;
}
.ft-apps__segment ion-segment-button ion-icon {
  font-size: 18px;
}
@media (min-width: 768px) {
  .ft-apps__segment ion-segment {
    max-width: 420px;
  }
}
.ft-apps__head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 18px 20px 0;
}
.ft-apps__rule {
  flex: 1;
  height: 1px;
  background: rgba(var(--ion-color-medium-rgb), 0.35);
}
.ft-apps__head h2 {
  margin: 0;
  color: var(--ion-color-medium);
  font-size: 12px;
  font-weight: 500;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}
.ft-apps__note {
  margin: 10px 20px 0;
  text-align: center;
  color: var(--ion-color-medium);
  font-size: 14px;
  line-height: 1.4;
}
</style>
