<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch, watchEffect } from "vue";
import {
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonPage,
  IonTitle,
  IonToolbar,
  onIonViewDidEnter,
} from "@ionic/vue";
import {
  alertCircleOutline,
  attachOutline,
  checkmark,
  checkmarkDone,
  checkmarkDoneCircle,
  chevronForwardOutline,
  ellipsisVerticalOutline,
  micOutline,
  peopleOutline,
  personAddOutline,
  qrCodeOutline,
  timeOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import ChatListToggle from "../components/ChatListToggle.vue";
import ChatThread from "../components/ChatThread.vue";
import ChatsEmpty from "../components/ChatsEmpty.vue";
import CircleThread from "../components/CircleThread.vue";
import QuickTour from "../components/QuickTour.vue";
import SessionActions from "../components/SessionActions.vue";
import { useChatListCollapse } from "../chat-list-collapse";
import { t } from "../i18n";
import { previewOf } from "../links";
import { store, type Chat, type Circle, type Session } from "../core";
import { useOnScreen } from "../on-screen";
import { showInPane, takeSearch } from "../pending-search";
import { isTourDue } from "../preferences";

const router = useRouter();

const wideQuery = window.matchMedia("(min-width: 768px)");
const wide = ref(wideQuery.matches);
const onWidthChange = (event: MediaQueryListEvent) => {
  wide.value = event.matches;
};
onMounted(() => wideQuery.addEventListener("change", onWidthChange));
onBeforeUnmount(() => wideQuery.removeEventListener("change", onWidthChange));
const { collapsed, toggle: toggleList } = useChatListCollapse(wide);

const touring = ref(false);
function offerTour() {
  if (isTourDue()) touring.value = true;
}

const onScreen = useOnScreen(offerTour);
// if a page covered the tabs before Chats came in, Ionic only says the tabs page is back
watch(onScreen, (now) => {
  if (now) offerTour();
});
onIonViewDidEnter(() => {
  if (wide.value && selectedId.value && !selectedCircle.value && takeSearch(selectedId.value)) paneThread.value?.openSearch();
});

const selectedId = ref(store.chats[0]?.id ?? "");
const selectedCircle = ref("");
const paneThread = ref<InstanceType<typeof ChatThread> | null>(null);
watchEffect(() => showInPane(wide.value && !selectedCircle.value ? selectedId.value : ""));
onBeforeUnmount(() => showInPane(""));

function open(id: string) {
  if (wide.value) {
    selectedId.value = id;
    selectedCircle.value = "";
  } else {
    router.push(`/chat/${id}`);
  }
}

function openCircle(id: string) {
  if (wide.value) {
    selectedCircle.value = id;
    selectedId.value = "";
  } else {
    router.push(`/circle/${id}`);
  }
}

function shown(preview: string): string {
  return previewOf(preview, t("chat.location"));
}

function circlePreview(circle: Circle): string {
  return circle.lastSender ? `${circle.lastSender}: ${shown(circle.preview)}` : shown(circle.preview);
}

const folded = ref<Record<string, boolean>>({});
const isFolded = (id: string) => folded.value[id] === true;
function fold(id: string) {
  folded.value = { ...folded.value, [id]: !isFolded(id) };
}

const keyOf = (session: Session) => session.id || `pin:${session.pin}`;
const empty = computed(() => !store.chats.length && !store.requests.length && !store.circles.length);

// the short id shows who a stranger really is
const shortId = (chat: Chat) => chat.id.slice(0, 9);
const count = (unread: number) => (unread > 99 ? "99+" : String(unread));

const STATUS_ICON: Record<string, string> = {
  pending: timeOutline,
  sent: checkmark,
  delivered: checkmarkDone,
  // read gets its own icon, Mono may share the accent colour
  read: checkmarkDoneCircle,
  unsent: alertCircleOutline,
};
</script>

<template>
  <ion-page class="ft-chats" :class="{ 'is-wide': wide, 'is-collapsed': collapsed }">
    <ion-header class="ion-no-border ft-chats__head">
      <ion-toolbar>
        <ion-buttons v-if="wide" slot="start">
          <ChatListToggle :collapsed="collapsed" controls="ft-chat-list" @toggle="toggleList" />
        </ion-buttons>
        <ion-title><span class="ft-title">{{ $t("tabs.chats") }}</span></ion-title>
        <ion-buttons slot="end">
          <ion-button v-if="store.chats.length" data-test="new-circle" :aria-label="$t('circle.new')" @click="router.push('/new-circle')">
            <ion-icon slot="icon-only" :icon="peopleOutline" aria-hidden="true" />
          </ion-button>
          <ion-button :aria-label="$t('addContact.title')" @click="router.push('/add-contact')">
            <ion-icon slot="icon-only" :icon="qrCodeOutline" aria-hidden="true" />
          </ion-button>
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content id="ft-chat-list" class="ft-chats__list" role="region" :aria-label="$t('tabs.chats')">
      <ion-header collapse="condense" class="ion-no-border">
        <ion-toolbar>
          <ion-title size="large"><span class="ft-title">{{ $t("tabs.chats") }}</span></ion-title>
        </ion-toolbar>
      </ion-header>

      <div>
        <ChatsEmpty v-if="empty" :compact="collapsed" data-test="empty" @tour="touring = true" />

        <section v-if="store.requests.length" class="ft-requests" data-test="requests" aria-labelledby="ft-requests-title" aria-describedby="ft-requests-hint">
          <ion-icon :icon="personAddOutline" class="ft-requests__mark" aria-hidden="true" />
          <h2 id="ft-requests-title" class="ft-requests__title">{{ $t("requests.title") }}</h2>
          <p id="ft-requests-hint" class="ft-requests__hint">{{ $t("requests.hint") }}</p>
          <ul class="ft-rows">
            <li v-for="chat in store.requests" :key="chat.id" data-test="request-row">
              <button type="button" class="ft-row" :class="{ 'is-selected': wide && chat.id === selectedId }" @click="open(chat.id)">
                <Avatar :name="chat.name" :hue="chat.hue" :connected="false" />
                <span class="ft-row__body">
                  <span class="ft-row__line">
                    <span class="ft-row__name" dir="auto">{{ chat.name }}</span>
                    <span class="ft-row__time">{{ shortId(chat) }}</span>
                  </span>
                  <span class="ft-row__line">
                    <span class="ft-row__preview" dir="auto">{{ shown(chat.preview) }}</span>
                  </span>
                </span>
              </button>
            </li>
          </ul>
        </section>

        <ul v-if="store.circles.length" class="ft-rows">
          <li v-for="one in store.circles" :key="one.id">
            <button
              type="button"
              class="ft-row"
              :class="{ 'is-selected': wide && one.id === selectedCircle }"
              data-test="circle-row"
              @click="openCircle(one.id)"
            >
              <Avatar :name="one.name" :hue="one.hue" />
              <span class="ft-row__body">
                <span class="ft-row__line">
                  <span class="ft-row__name" dir="auto">{{ one.name }}</span>
                  <span class="ft-row__time" :class="{ 'is-unread': one.unread }">{{ one.time }}</span>
                </span>
                <span class="ft-row__line">
                  <ion-icon :icon="peopleOutline" class="ft-row__kind ft-row__circle" role="img" :aria-label="$t('circle.title')" />
                  <ion-icon
                    v-if="one.lastMine && one.status"
                    :icon="STATUS_ICON[one.status]"
                    class="ft-row__status"
                    :class="`is-${one.status}`"
                    aria-hidden="true"
                  />
                  <span class="ft-row__preview" dir="auto">{{ circlePreview(one) }}</span>
                  <span v-if="one.unread" class="ft-row__badge" data-test="unread">{{ count(one.unread) }}</span>
                </span>
              </span>
            </button>
            <button
              type="button"
              class="ft-row__more"
              data-test="circle-more"
              :aria-label="$t('circle.settings')"
              @click="router.push(`/circle/${one.id}/info`)"
            >
              <ion-icon :icon="ellipsisVerticalOutline" aria-hidden="true" />
            </button>
          </li>
        </ul>

        <ul v-if="store.chats.length" class="ft-rows">
          <li v-for="chat in store.chats" :key="chat.id">
            <button
              type="button"
              class="ft-row"
              :class="{ 'is-selected': wide && chat.id === selectedId }"
              data-test="chat-row"
              @click="open(chat.id)"
            >
              <Avatar :name="chat.name" :hue="chat.hue" :connected="chat.connected" />
              <span class="ft-row__body">
                <span class="ft-row__line">
                  <span class="ft-row__name" dir="auto">{{ chat.name }}</span>
                  <span class="ft-row__time" :class="{ 'is-unread': chat.unread }">{{ chat.time }}</span>
                </span>
                <span class="ft-row__line">
                  <ion-icon
                    v-if="chat.lastMine"
                    :icon="STATUS_ICON[chat.status]"
                    class="ft-row__status"
                    :class="`is-${chat.status}`"
                    aria-hidden="true"
                  />
                  <ion-icon
                    v-if="chat.lastKind !== 'text'"
                    :icon="chat.lastKind === 'voice' ? micOutline : attachOutline"
                    class="ft-row__kind"
                    role="img"
                    :aria-label="chat.lastKind === 'voice' ? $t('chat.voiceMessage') : $t('chat.file')"
                  />
                  <span class="ft-row__preview" dir="auto">{{ chat.lastKind === "voice" ? $t("chat.voiceMessage") : shown(chat.preview) }}</span>
                  <span v-if="chat.unread" class="ft-row__badge" data-test="unread">{{ count(chat.unread) }}</span>
                </span>
              </span>
            </button>
            <button
              type="button"
              class="ft-row__more"
              data-test="chat-more"
              :aria-label="$t('chat.contactSettings')"
              @click="router.push(`/contact/${chat.id}`)"
            >
              <ion-icon :icon="ellipsisVerticalOutline" aria-hidden="true" />
            </button>
          </li>
        </ul>

        <!-- no session names, closed sessions leave no trace -->
        <section
          v-for="session in store.sessions"
          :key="keyOf(session)"
          class="ft-session"
          :class="{ 'is-folded': isFolded(session.id) }"
          data-test="session-section"
        >
          <header class="ft-session__head">
            <button
              type="button"
              class="ft-session__toggle"
              data-test="session-toggle"
              :aria-label="$t('session.toggle')"
              :aria-expanded="!isFolded(session.id)"
              @click="fold(session.id)"
            >
              <ion-icon :icon="chevronForwardOutline" class="ft-session__chevron" aria-hidden="true" />
            </button>
            <SessionActions v-if="!collapsed" :session="session" />
          </header>

          <template v-if="!isFolded(session.id)">
            <ion-icon v-if="session.requests.length" :icon="personAddOutline" class="ft-requests__mark" aria-hidden="true" />
            <ul v-if="session.requests.length" class="ft-rows ft-requests__rows" data-test="session-requests">
              <li v-for="chat in session.requests" :key="chat.id" data-test="request-row">
                <button type="button" class="ft-row" :class="{ 'is-selected': wide && chat.id === selectedId }" @click="open(chat.id)">
                  <Avatar :name="chat.name" :hue="chat.hue" :connected="false" />
                  <span class="ft-row__body">
                    <span class="ft-row__line">
                      <span class="ft-row__name" dir="auto">{{ chat.name }}</span>
                      <span class="ft-row__time">{{ shortId(chat) }}</span>
                    </span>
                    <span class="ft-row__line"><span class="ft-row__preview" dir="auto">{{ shown(chat.preview) }}</span></span>
                  </span>
                </button>
              </li>
            </ul>
            <ul v-if="session.circles.length" class="ft-rows">
              <li v-for="one in session.circles" :key="one.id">
                <button
                  type="button"
                  class="ft-row"
                  :class="{ 'is-selected': wide && one.id === selectedCircle }"
                  data-test="circle-row"
                  @click="openCircle(one.id)"
                >
                  <Avatar :name="one.name" :hue="one.hue" />
                  <span class="ft-row__body">
                    <span class="ft-row__line">
                      <span class="ft-row__name" dir="auto">{{ one.name }}</span>
                      <span class="ft-row__time" :class="{ 'is-unread': one.unread }">{{ one.time }}</span>
                    </span>
                    <span class="ft-row__line">
                      <ion-icon :icon="peopleOutline" class="ft-row__kind ft-row__circle" role="img" :aria-label="$t('circle.title')" />
                      <span class="ft-row__preview" dir="auto">{{ circlePreview(one) }}</span>
                      <span v-if="one.unread" class="ft-row__badge" data-test="unread">{{ count(one.unread) }}</span>
                    </span>
                  </span>
                </button>
                <button
                  type="button"
                  class="ft-row__more"
                  data-test="circle-more"
                  :aria-label="$t('circle.settings')"
                  @click="router.push(`/circle/${one.id}/info`)"
                >
                  <ion-icon :icon="ellipsisVerticalOutline" aria-hidden="true" />
                </button>
              </li>
            </ul>
            <p v-if="!session.chats.length && !session.circles.length" class="ft-session__empty">{{ $t("session.empty") }}</p>
            <ul v-else class="ft-rows">
              <li v-for="chat in session.chats" :key="chat.id">
                <button
                  type="button"
                  class="ft-row"
                  :class="{ 'is-selected': wide && chat.id === selectedId }"
                  data-test="chat-row"
                  @click="open(chat.id)"
                >
                  <Avatar :name="chat.name" :hue="chat.hue" :connected="chat.connected" />
                  <span class="ft-row__body">
                    <span class="ft-row__line">
                      <span class="ft-row__name" dir="auto">{{ chat.name }}</span>
                      <span class="ft-row__time" :class="{ 'is-unread': chat.unread }">{{ chat.time }}</span>
                    </span>
                    <span class="ft-row__line">
                      <ion-icon
                        v-if="chat.lastMine"
                        :icon="STATUS_ICON[chat.status]"
                        class="ft-row__status"
                        :class="`is-${chat.status}`"
                        aria-hidden="true"
                      />
                      <span class="ft-row__preview" dir="auto">{{ chat.lastKind === "voice" ? $t("chat.voiceMessage") : shown(chat.preview) }}</span>
                      <span v-if="chat.unread" class="ft-row__badge" data-test="unread">{{ count(chat.unread) }}</span>
                    </span>
                  </span>
                </button>
                <button
                  type="button"
                  class="ft-row__more"
                  data-test="chat-more"
                  :aria-label="$t('chat.contactSettings')"
                  @click="router.push(`/contact/${chat.id}`)"
                >
                  <ion-icon :icon="ellipsisVerticalOutline" aria-hidden="true" />
                </button>
              </li>
            </ul>
          </template>
        </section>
      </div>
    </ion-content>

    <section v-if="wide && selectedCircle" class="ft-chats__detail">
      <div class="ft-thread">
        <CircleThread :circle-id="selectedCircle" :active="onScreen" />
      </div>
    </section>
    <section v-else-if="wide && selectedId" class="ft-chats__detail">
      <div class="ft-thread">
        <ChatThread ref="paneThread" :chat-id="selectedId" split :active="onScreen" />
      </div>
    </section>

    <QuickTour class="ft-sheet--tab" :open="touring && onScreen" @close="touring = false" />
  </ion-page>
</template>

<style scoped>
.ft-chats.is-wide > .ft-chats__head,
.ft-chats.is-wide > .ft-chats__list {
  width: var(--ft-list-width);
  border-inline-end: 1px solid var(--ft-border);
}
.ft-chats__detail {
  position: absolute;
  top: 0;
  bottom: 0;
  inset-inline-start: var(--ft-list-width);
  inset-inline-end: 0;
}
.ft-thread {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  background: var(--ft-bg);
}

.ft-row {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  /* without min-width: 0 the preview pushes the ⋮ off screen */
  min-width: 0;
  padding: 10px;
  border: 0;
  border-radius: 16px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
  transition: background 0.15s;
}
@media (hover: hover) {
  .ft-row:hover {
    background: color-mix(in srgb, var(--ft-surface-2) 60%, transparent);
  }
  .ft-row.is-selected:hover {
    background: color-mix(in srgb, var(--ft-accent) 17%, transparent);
  }
}
.ft-row.is-selected {
  background: color-mix(in srgb, var(--ft-accent) 12%, transparent);
}
.ft-row:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: -2px;
}

.ft-row__body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
}
.ft-row__line {
  display: flex;
  align-items: center;
  gap: 6px;
}
.ft-row__name {
  flex: 1;
  font-size: 16px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-row__time {
  font-size: 12px;
  color: var(--ft-muted);
  font-variant-numeric: tabular-nums;
}
.ft-row__time.is-unread {
  color: var(--ft-accent);
  font-weight: 600;
}
.ft-row__status {
  flex-shrink: 0;
  font-size: 16px;
  color: var(--ft-muted);
}
.ft-row__status.is-read {
  color: var(--ft-accent);
}
.ft-row__status.is-unsent {
  color: var(--ion-color-danger);
}
.ft-row__kind {
  flex-shrink: 0;
  font-size: 15px;
  color: var(--ft-muted);
}
.ft-row__preview {
  flex: 1;
  font-size: 14px;
  color: var(--ft-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.ft-rows li {
  display: flex;
  align-items: center;
}
.ft-row__more {
  appearance: none;
  flex: none;
  display: grid;
  place-items: center;
  width: 40px;
  align-self: stretch;
  border: 0;
  background: transparent;
  color: var(--ft-muted);
  font-size: 18px;
  cursor: pointer;
}

.ft-requests {
  padding: 4px 0 8px;
  border-bottom: 1px solid var(--ft-border);
}
.ft-requests__title {
  margin: 8px 18px 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--ft-accent);
}
.ft-requests__hint {
  margin: 2px 18px 6px;
  font-size: 12px;
  color: var(--ft-muted);
  line-height: 1.35;
}
.ft-session {
  border-top: 1px solid var(--ft-border);
}
.ft-session .ft-rows {
  padding-bottom: 8px;
}
.ft-session__head {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 6px 8px 6px 10px;
}
.ft-session__toggle {
  appearance: none;
  display: flex;
  flex: 1;
  align-items: center;
  min-width: 0;
  min-height: 36px;
  padding: 8px 6px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.ft-session__toggle:hover {
  background: color-mix(in srgb, var(--ft-surface-2) 60%, transparent);
}
.ft-session__chevron {
  flex-shrink: 0;
  font-size: 16px;
  color: var(--ft-muted);
  transform: rotate(90deg);
  transition: transform 0.15s;
}
.ft-session.is-folded .ft-session__chevron {
  transform: none;
}
.ft-session__empty {
  margin: 0;
  padding: 6px 20px 16px;
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.4;
}

.ft-row__badge {
  display: grid;
  place-items: center;
  min-width: 20px;
  height: 20px;
  padding: 0 6px;
  border-radius: 10px;
  background: var(--ft-accent);
  color: var(--ft-on-accent);
  font-size: 12px;
  font-weight: 700;
  box-shadow: 0 0 10px var(--ft-glow);
}

.ft-requests__mark {
  display: none;
}
.ft-session__toggle:focus-visible {
  outline: 2px solid var(--ft-accent);
  outline-offset: 2px;
}
[dir="rtl"] .ft-session.is-folded .ft-session__chevron {
  transform: scaleX(-1);
}

.ft-chats.is-collapsed ion-title,
.ft-chats.is-collapsed ion-buttons[slot="end"],
.ft-chats.is-collapsed .ft-chats__list > ion-header,
.ft-chats.is-collapsed .ft-requests__hint,
.ft-chats.is-collapsed .ft-row__time,
.ft-chats.is-collapsed .ft-row__status,
.ft-chats.is-collapsed .ft-row__kind:not(.ft-row__circle),
.ft-chats.is-collapsed .ft-row__preview,
.ft-chats.is-collapsed .ft-row__more,
.ft-chats.is-collapsed .ft-session__empty {
  display: none;
}
.ft-chats.is-collapsed .ft-rows {
  padding: 4px 4px 14px;
}
.ft-chats.is-collapsed :is(.ft-requests, .ft-session) .ft-rows {
  padding-top: 0;
}
.ft-chats.is-collapsed .ft-rows li {
  display: block;
}
.ft-chats.is-collapsed .ft-row {
  position: relative;
  flex-direction: column;
  justify-content: center;
  gap: 6px;
  min-height: 86px;
  padding: 8px 4px;
  border-radius: 14px;
}
.ft-chats.is-collapsed .ft-row__body,
.ft-chats.is-collapsed .ft-row__line {
  display: contents;
}
.ft-chats.is-collapsed .ft-row__name {
  display: block;
  flex: none;
  width: 100%;
  max-width: 72px;
  font-size: 11px;
  line-height: 16px;
  text-align: center;
}
.ft-chats.is-collapsed .ft-row:has(.ft-row__badge) .ft-row__name {
  font-weight: 700;
}
.ft-chats.is-collapsed .ft-row__badge {
  position: absolute;
  top: 4px;
  inset-inline-end: 6px;
  padding: 0 5px;
  border: 2px solid var(--ft-bg);
  font-size: 11px;
  line-height: 16px;
  font-variant-numeric: tabular-nums;
}
.ft-chats.is-collapsed .ft-row__circle {
  position: absolute;
  top: 39px;
  inset-inline-start: 8px;
  width: 12px;
  height: 12px;
  padding: 2px;
  border: 2px solid var(--ft-bg);
  border-radius: 50%;
  background: var(--ft-surface);
  color: var(--ft-accent);
}
.ft-chats.is-collapsed .ft-row.is-selected::before {
  content: "";
  position: absolute;
  inset-block: 12px;
  inset-inline-start: 2px;
  width: 3px;
  border-radius: 3px;
  background: var(--ft-accent);
}
.ft-chats.is-collapsed .ft-requests {
  padding-top: 4px;
}
.ft-chats.is-collapsed .ft-requests__mark {
  display: block;
  margin: 7px auto;
  font-size: 18px;
  color: var(--ft-accent);
}
.ft-chats.is-collapsed .ft-requests__title {
  position: absolute;
  width: 1px;
  height: 1px;
  margin: -1px;
  overflow: hidden;
  clip-path: inset(50%);
  white-space: nowrap;
}
.ft-chats.is-collapsed .ft-session {
  background: color-mix(in srgb, var(--ft-surface-2) 45%, transparent);
}
.ft-chats.is-collapsed .ft-session__head {
  justify-content: center;
  min-height: 48px;
  padding: 2px 0;
}
.ft-chats.is-collapsed .ft-session__toggle {
  flex: none;
  justify-content: center;
  width: 44px;
  height: 44px;
  padding: 0;
}
.ft-chats.is-collapsed .ft-session__chevron {
  font-size: 18px;
}
</style>
