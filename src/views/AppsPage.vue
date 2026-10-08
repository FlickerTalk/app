<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import {
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonLabel,
  IonList,
  IonListHeader,
  IonModal,
  IonPage,
  IonSegment,
  IonSegmentButton,
  IonTitle,
  IonToast,
  IonToolbar,
  onIonViewDidEnter,
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { constructOutline, gameControllerOutline, lockClosedOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import AppSheet from "../components/AppSheet.vue";
import AppTile from "../components/AppTile.vue";
import Avatar from "../components/Avatar.vue";
import GamePermissions from "../components/GamePermissions.vue";
import {
  formatSize,
  grantPlugin,
  installPlugin,
  needsSubscription,
  PREMIUM_PAGE,
  removePlugin,
  store,
  type OfferedPlugin,
  type PluginView,
} from "../core";
import { gameGrant, isGame, needsGameGrant } from "../games";
import { permissionsOf, withPermission } from "../permissions";
import {
  byPluginName,
  games,
  installed,
  isLocked,
  offered,
  pluginIcon,
  pluginName,
  refreshOffered,
  refreshPlugins,
  refreshPremiumLock,
  tools,
} from "../plugins";
import { closeOnBackWhile } from "../back";

// 2026-10-08 (plan of the apps grid, docs/mockups/apps-grid.html): the tools and the games of this
// phone as a grid of icons, and under them what the app carries or the catalogue offers. A tap
// opens (a tool), asks who to play with (a game) or installs; a hold shows what the app is.
// Installing grants nothing (§53): its permissions are switches on its sheet.
const router = useRouter();
const route = useRoute();

type Segment = "tools" | "games";
/** The segment shown; Ionic keeps the tab mounted, so it is remembered while the app runs. */
const segment = ref<Segment>(route.query?.show === "games" ? "games" : "tools");
// The chat's apps sheet asks for the games ("More games") and Settings' Tools row for the tools,
// also when the tab is already mounted and was left on the other segment.
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
/** What the catalogue and the app offer of this kind; what is installed is not offered again. */
const ofKind = computed(() => offered.value.filter((one) => isGame(one) === onGames.value));
const more = computed(() =>
  byPluginName(ofKind.value.filter((one) => !one.installed && !installed.value.some((here) => here.id === one.id))),
);

// What is shown appears once it is known, never replaced by something of another height: the
// installed grid once the core said what is installed, the "more" part once the catalogue answered.
const pluginsRead = ref(false);
const offeredRead = ref(false);

/** The app being installed now, and whether the last install failed (a toast says so). */
const installing = ref("");
const failed = ref(false);

/** The app whose sheet is open (a hold on its tile), by id: it shows what the core says now. */
const sheetId = ref("");
const sheetOpen = ref(false);
const sheetPlugin = computed<PluginView | OfferedPlugin | null>(
  () => installed.value.find((one) => one.id === sheetId.value) ?? offered.value.find((one) => one.id === sheetId.value) ?? null,
);

/** The game whose permissions sheet is open (plan decision 11), and the one looking for a contact. */
const asking = ref<PluginView | null>(null);
const picking = ref("");

// Only while the tab is on screen: Ionic keeps it mounted under the others (2026-10-02).
const onScreen = ref(true);
onIonViewDidEnter(() => (onScreen.value = true));
onIonViewWillLeave(() => {
  onScreen.value = false;
  // What was open on top is forgotten on the way out.
  sheetOpen.value = false;
  asking.value = null;
  picking.value = "";
});
closeOnBackWhile(() => onScreen.value && sheetOpen.value, () => (sheetOpen.value = false));
closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => (asking.value = null));
closeOnBackWhile(() => onScreen.value && Boolean(picking.value), () => (picking.value = ""));

onMounted(refresh);
// A game installed from a chat's invitation shows up when the tab comes back.
onIonViewWillEnter(refresh);

async function refresh() {
  await Promise.all([refreshPlugins(), refreshPremiumLock()]);
  pluginsRead.value = true;
  // Offline the core answers with what the app carries.
  await refreshOffered().catch(() => undefined);
  offeredRead.value = true;
}

/** Ioan, 2026-10-08: a locked tool goes to the Premium section of Settings, where the subscription is. */
function subscribe() {
  void router.push(PREMIUM_PAGE);
}

function badgeOf(one: PluginView | OfferedPlugin): "download" | "add" | "lock" | undefined {
  if (isLocked(one)) return "lock";
  if ("asks" in one) return undefined;
  return one.carried ? "add" : "download";
}

/** What a download weighs; what the app carries costs nothing and says nothing (§52). */
function weight(one: OfferedPlugin): string {
  return one.carried ? "" : formatSize(one.size);
}

/** A tap on an installed app: a tool opens on its own; a game asks who to play with. */
function use(one: PluginView) {
  if (isGame(one)) return playGame(one);
  if (isLocked(one)) return subscribe();
  void router.push(`/plugin/${one.id}`);
}

/** Nothing arrives installed: the user picks the app, and it starts with no permission (§53). */
async function install(one: OfferedPlugin) {
  if (isLocked(one)) return subscribe();
  if (installing.value) return;
  installing.value = one.id;
  failed.value = false;
  try {
    await installPlugin(one.id);
  } catch (error) {
    // The plan closed while the tab was open (the free days ended): the core refused it.
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

// QA of 1.4.0 (2026-10-06): two switches turned quickly lost the first, because the second grant was
// built from the plugin as it was before either. The changes go one at a time, each from what the
// core says now, and the switch ends showing what the core has, even if it refused the change.
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
  });
  return changes;
}

/** ▶️: first what the game needs, once; then who to play with. */
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

/**
 * Who a game can be played with: the contacts of the main list and of each open hidden session
 * (§108), never a blocked one. Strangers who wrote first are not contacts yet.
 */
const places = computed(() =>
  [
    { id: "", chats: store.chats },
    ...store.sessions.filter((session) => session.id).map((session) => ({ id: session.id, chats: session.chats })),
  ]
    .map((place) => ({ id: place.id, chats: place.chats.filter((chat) => !chat.blocked) }))
    .filter((place) => place.chats.length),
);

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
          :badge="badgeOf(one)"
          :data-test="`app-${one.id}`"
          @tap="use(one)"
          @hold="hold(one)"
        />
      </div>
      <p v-else-if="pluginsRead" class="ft-apps__note" data-test="apps-none">
        {{ onGames ? $t("apps.noGames") : $t("apps.noTools") }}
      </p>

      <!-- What this phone does not have yet: carried by the app (added) or a download (§56). -->
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

    <!-- A failed install floats over the grid: nothing in it moves (no layout shift). -->
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

    <!-- On a wide screen the tab's sheets cover the tab, everything after the rail (2026-10-02). -->
    <GamePermissions
      class="ft-sheet--tab"
      :open="Boolean(asking)"
      :name="asking ? pluginName(asking) : ''"
      :icon="asking ? pluginIcon(asking) : undefined"
      @allow="allow"
      @cancel="asking = null"
    />

    <!-- Plan 10.4: a game is played in a conversation; this is who with. -->
    <ion-modal
      :is-open="Boolean(picking)"
      class="ft-apps__picker ft-sheet--tab"
      :breakpoints="[0, 0.5, 1]"
      :initial-breakpoint="0.5"
      :expand-to-scroll="false"
      :aria-label="$t('games.pickContact')"
      @did-dismiss="picking = ''"
    >
      <ion-header>
        <ion-toolbar>
          <ion-title><span class="ft-title">{{ $t("games.pickContact") }}</span></ion-title>
        </ion-toolbar>
      </ion-header>
      <ion-content class="ft-apps__picker-content" data-test="contact-picker">
        <ion-list v-if="!places.length">
          <ion-item lines="none">
            <ion-label color="medium">{{ $t("games.noContacts") }}</ion-label>
          </ion-item>
        </ion-list>
        <ion-list v-for="place in places" :key="place.id">
          <!-- A hidden session's contacts, under their own heading. -->
          <ion-list-header v-if="place.id">
            <ion-icon :icon="lockClosedOutline" aria-hidden="true" />
          </ion-list-header>
          <ion-item v-for="chat in place.chats" :key="chat.id" button :detail="false" :data-test="`play-with-${chat.id}`" @click="playWith(chat.id)">
            <Avatar slot="start" :name="chat.name" :hue="chat.hue" :size="36" />
            <ion-label class="ion-text-nowrap" dir="auto">{{ chat.name }}</ion-label>
          </ion-item>
        </ion-list>
      </ion-content>
    </ion-modal>
  </ion-page>
</template>

<style scoped>
/* The last row scrolls above Android's navigation bar (edge to edge). */
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
/* The picker's list scrolls to its last contact above Android's navigation bar (edge to edge). */
.ft-apps__picker-content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
</style>
