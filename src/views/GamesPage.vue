<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import {
  IonContent,
  IonHeader,
  IonIcon,
  IonItem,
  IonButton,
  IonLabel,
  IonList,
  IonListHeader,
  IonModal,
  IonPage,
  IonTitle,
  IonToggle,
  IonToolbar,
  onIonViewDidEnter,
  onIonViewWillEnter,
  onIonViewWillLeave,
} from "@ionic/vue";
import { downloadOutline, gameControllerOutline, lockClosedOutline, play, trashOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import GamePermissions from "../components/GamePermissions.vue";
import { formatSize, grantPlugin, installPlugin, removePlugin, store, type PluginView } from "../core";
import { gameGrant, needsGameGrant } from "../games";
import { permissionsOf, withPermission } from "../permissions";
import { games, offeredGames, pluginName, pluginSummary, refreshOffered, refreshPlugins } from "../plugins";
import { closeOnBackWhile } from "../back";
import { t } from "../i18n";

// Plan 10.4: the games of this phone and those the catalogue offers. A game is a plugin like any
// other: installing grants nothing (§53), its permissions are switches that can be taken back, and
// it is played inside a conversation, with the contact picked here.
const router = useRouter();

/** What the catalogue offers that this phone does not have yet. */
const more = computed(() => offeredGames.value.filter((one) => !one.installed));
const error = ref("");
const asksToRemove = ref("");
/** The game whose permissions sheet is open (decision 11), and the one looking for a contact. */
const asking = ref<PluginView | null>(null);
const picking = ref("");

// Only while the tab is on screen: Ionic keeps it mounted under the others (2026-10-02).
const onScreen = ref(true);
onIonViewWillLeave(() => (onScreen.value = false));
onIonViewDidEnter(() => (onScreen.value = true));
closeOnBackWhile(() => onScreen.value && Boolean(asking.value), () => (asking.value = null));
closeOnBackWhile(() => onScreen.value && Boolean(picking.value), () => (picking.value = ""));

onMounted(refresh);
// A game installed from a chat's invitation shows up when the tab comes back.
onIonViewWillEnter(refresh);
// Ionic keeps the tab mounted behind the others: a question left open is forgotten on the way out.
onIonViewWillLeave(() => (asksToRemove.value = ""));

/** 🗑️ asks about this game, or stops asking when tapped again; one game at a time. */
function askToRemove(id: string) {
  asksToRemove.value = asksToRemove.value === id ? "" : id;
}

async function refresh() {
  await refreshPlugins();
  // The core answers offline with what the app carries: no game, which the page says honestly.
  await refreshOffered().catch(() => undefined);
}

async function install(id: string) {
  error.value = "";
  try {
    await installPlugin(id);
  } catch {
    error.value = t("games.installFailed");
  }
  await refresh();
}

async function toggle(game: PluginView, key: string, on: boolean) {
  await grantPlugin(game.id, withPermission(game, key, on));
  await refresh();
}

/** Its records go with it: the saved games of every place on this phone (plan 10.4). */
async function remove(id: string) {
  asksToRemove.value = "";
  await removePlugin(id);
  await refresh();
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
        <ion-title><span class="ft-title">{{ $t("tabs.games") }}</span></ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <p v-if="error" class="ft-games__error" role="alert">{{ error }}</p>

      <h2 class="ft-games__title">{{ $t("games.mine") }}</h2>
      <div v-if="games.length" data-test="my-games">
        <ion-list v-for="game in games" :key="game.id" inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="gameControllerOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ pluginName(game) }}
              <p class="ft-muted">{{ game.version }}</p>
            </ion-label>
            <ion-button slot="end" fill="clear" size="default" :data-test="`play-${game.id}`" :aria-label="$t('games.play')" @click="playGame(game)">
              <ion-icon slot="icon-only" :icon="play" aria-hidden="true" />
            </ion-button>
            <ion-button
              slot="end"
              fill="clear"
              size="default"
              color="danger"
              :data-test="`remove-${game.id}`"
              :aria-label="$t('games.remove')"
              :aria-pressed="asksToRemove === game.id"
              @click="askToRemove(game.id)"
            >
              <ion-icon slot="icon-only" :icon="trashOutline" aria-hidden="true" />
            </ion-button>
          </ion-item>
          <!-- Removing deletes its saved games: asked once, as Settings asks before erasing, with a
               way back. -->
          <ion-item v-if="asksToRemove === game.id" lines="none">
            <ion-label class="ft-games__warning">{{ $t("games.removeWarning") }}</ion-label>
            <ion-button slot="end" class="ft-games__choice" fill="outline" shape="round" size="small" data-test="remove-cancel" @click="asksToRemove = ''">
              {{ $t("common.cancel") }}
            </ion-button>
            <ion-button slot="end" class="ft-games__choice" shape="round" size="small" color="danger" data-test="remove-confirm" @click="remove(game.id)">
              {{ $t("games.remove") }}
            </ion-button>
          </ion-item>

          <ion-item v-for="permission in permissionsOf(game)" :key="permission.key" lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="permission.icon" aria-hidden="true" /></span>
            <ion-label>{{ permission.label }}</ion-label>
            <ion-toggle
              slot="end"
              :checked="permission.on"
              :aria-label="permission.label"
              @ion-change="toggle(game, permission.key, $event.detail.checked)"
            />
          </ion-item>
        </ion-list>
      </div>
      <p v-else class="ft-games__hint">{{ $t("games.none") }}</p>

      <!-- What the catalogue offers and this phone does not have yet: a download (§56). -->
      <template v-if="more.length">
        <h2 class="ft-games__title">{{ $t("games.more") }}</h2>
        <ion-list inset class="ft-group" data-test="more-games">
          <ion-item v-for="one in more" :key="one.id" lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="gameControllerOutline" aria-hidden="true" /></span>
            <ion-label>
              {{ pluginName(one) }}
              <p class="ft-muted">{{ pluginSummary(one) }} · {{ formatSize(one.size) }}</p>
            </ion-label>
            <ion-button slot="end" fill="clear" size="default" :data-test="`install-${one.id}`" :aria-label="$t('games.install')" @click="install(one.id)">
              <ion-icon slot="icon-only" :icon="downloadOutline" aria-hidden="true" />
            </ion-button>
          </ion-item>
        </ion-list>
      </template>
      <p v-else-if="!offeredGames.length" class="ft-games__hint">{{ $t("games.offline") }}</p>
    </ion-content>

    <!-- On a wide screen the tab's sheets cover the tab, everything after the rail (2026-10-02). -->
    <GamePermissions class="ft-sheet--tab" :open="Boolean(asking)" :name="asking ? pluginName(asking) : ''" @allow="allow" @cancel="asking = null" />

    <!-- Plan 10.4: a game is played in a conversation; this is who with. Ionic's sheet modal, as the
         apps sheet: its list scrolls at any height, and it goes by its handle, a tap outside or Back. -->
    <ion-modal
      :is-open="Boolean(picking)"
      class="ft-games__picker ft-sheet--tab"
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
      <ion-content class="ft-games__picker-content" data-test="contact-picker">
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
.ft-games__title {
  margin: var(--ft-space-5) var(--ft-space-4) 0;
  font-size: 15px;
  font-weight: 600;
  color: var(--ft-muted);
}
.ft-games__hint {
  margin: var(--ft-space-4);
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.4;
}
.ft-games__error {
  margin: var(--ft-space-4);
  color: var(--ion-color-danger);
  font-size: 14px;
}
/* The app's buttons say things as written, not in Material's capitals. */
.ft-games__choice {
  text-transform: none;
}
.ft-games__warning {
  color: var(--ion-color-danger);
  font-size: 13px;
  white-space: normal;
}
/* The picker's list scrolls to its last contact above Android's navigation bar (edge to edge), as
   the apps sheet does: Ionic pads a footer for it, not a content. */
.ft-games__picker-content {
  --padding-bottom: var(--ion-safe-area-bottom, 0px);
}
</style>
