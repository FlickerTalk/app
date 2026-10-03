<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
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
  onIonViewWillLeave,
} from "@ionic/vue";
import {
  alertCircleOutline,
  attachOutline,
  checkmark,
  checkmarkDone,
  checkmarkOutline,
  chevronForwardOutline,
  ellipsisVerticalOutline,
  logOutOutline,
  micOutline,
  peopleOutline,
  qrCodeOutline,
  timeOutline,
  trashOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import ChatThread from "../components/ChatThread.vue";
import CircleThread from "../components/CircleThread.vue";
import { t } from "../i18n";
import { previewOf } from "../links";
import { closeSession, removeSession, store, type Chat, type Circle, type Session } from "../core";

const router = useRouter();

const wideQuery = window.matchMedia("(min-width: 768px)");
const wide = ref(wideQuery.matches);
const onWidthChange = (event: MediaQueryListEvent) => {
  wide.value = event.matches;
};
onMounted(() => wideQuery.addEventListener("change", onWidthChange));
onBeforeUnmount(() => wideQuery.removeEventListener("change", onWidthChange));

// On another tab, what the conversation beside the list left open lets go of Android's back
// button; back on Chats it takes it again (2026-10-02).
const onScreen = ref(true);
onIonViewWillLeave(() => (onScreen.value = false));
onIonViewDidEnter(() => (onScreen.value = true));

const selectedId = ref(store.chats[0]?.id ?? "");
// Circles (2026-09-27) sit in the same list; on a wide screen one opens next to it like a chat.
const selectedCircle = ref("");

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
  } else {
    router.push(`/circle/${id}`);
  }
}

/** A last message as the list says it: a place in words (`📍 Location`), not its geo URI. */
function shown(preview: string): string {
  return previewOf(preview, t("chat.location"));
}

/** What the list says a circle's last line was: who said it, then the text. */
function circlePreview(circle: Circle): string {
  return circle.lastSender ? `${circle.lastSender}: ${shown(circle.preview)}` : shown(circle.preview);
}

/** A new circle of the main list, or of a session that exists in the core. */
function newCircle(session?: Session) {
  if (session && !session.id) return;
  router.push(session ? `/new-circle?session=${session.id}` : "/new-circle");
}

// Hidden sessions fold like the panels of a sidebar; each remembers whether it is open.
const folded = ref<Record<string, boolean>>({});
const isFolded = (id: string) => folded.value[id] === true;
function fold(id: string) {
  folded.value = { ...folded.value, [id]: !isFolded(id) };
}

/** A session that is only on the screen (A3: the seven slots are taken) has no room for anyone. */
function addTo(session: Session) {
  if (session.id) router.push(`/add-contact?session=${session.id}`);
}

// A3: deleting a session is for good, so it asks once, in the header, with no words. Every
// session has the trash; one that is only on the screen just goes from it.
const removing = ref<string | undefined>();
const keyOf = (session: Session) => session.id || `pin:${session.pin}`;

async function remove(session: Session) {
  removing.value = undefined;
  if (session.id) await removeSession(session.id);
  else await closeSession("");
}

// A5: strangers who wrote first wait here; a short id next to the name says who they really are.
const shortId = (chat: Chat) => chat.id.slice(0, 9);

const STATUS_ICON: Record<string, string> = {
  pending: timeOutline,
  sent: checkmark,
  delivered: checkmarkDone,
  read: checkmarkDone,
  unsent: alertCircleOutline,
};
</script>

<template>
  <ion-page>
    <div class="ft-chats" :class="{ 'is-wide': wide }">
      <section class="ft-chats__list">
        <ion-header class="ion-no-border">
          <ion-toolbar>
            <ion-title><span class="ft-title">{{ $t("tabs.chats") }}</span></ion-title>
            <ion-buttons slot="end">
              <ion-button v-if="store.chats.length" data-test="new-circle" :aria-label="$t('circle.new')" @click="newCircle()">
                <ion-icon slot="icon-only" :icon="peopleOutline" aria-hidden="true" />
              </ion-button>
              <ion-button :aria-label="$t('addContact.title')" @click="router.push('/add-contact')">
                <ion-icon slot="icon-only" :icon="qrCodeOutline" aria-hidden="true" />
              </ion-button>
            </ion-buttons>
          </ion-toolbar>
        </ion-header>

        <ion-content>
          <ion-header collapse="condense" class="ion-no-border">
            <ion-toolbar>
              <ion-title size="large"><span class="ft-title">{{ $t("tabs.chats") }}</span></ion-title>
            </ion-toolbar>
          </ion-header>

          <div v-if="!store.chats.length && !store.requests.length && !store.circles.length" class="ft-empty" data-test="empty">
            <p class="ft-empty__title">{{ $t("chats.empty") }}</p>
            <p class="ft-empty__hint">{{ $t("chats.emptyHint") }}</p>
            <button type="button" class="ft-empty__action" @click="router.push('/add-contact')">
              <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
              {{ $t("addContact.title") }}
            </button>
          </div>

          <!-- A5: the requests come first, apart from the list; the yes and the no are in the conversation (2026-09-28). -->
          <section v-if="store.requests.length" class="ft-requests" data-test="requests">
            <h2 class="ft-requests__title">{{ $t("requests.title") }}</h2>
            <p class="ft-requests__hint">{{ $t("requests.hint") }}</p>
            <ul class="ft-rows">
              <li v-for="chat in store.requests" :key="chat.id" data-test="request-row">
                <button type="button" class="ft-row" @click="open(chat.id)">
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

          <!-- Circles (2026-09-27): the same rows, with who said the last thing. -->
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
                    <ion-icon :icon="peopleOutline" class="ft-row__kind" role="img" :aria-label="$t('circle.title')" />
                    <ion-icon
                      v-if="one.lastMine && one.status"
                      :icon="STATUS_ICON[one.status]"
                      class="ft-row__status"
                      :class="`is-${one.status}`"
                      aria-hidden="true"
                    />
                    <span class="ft-row__preview" dir="auto">{{ circlePreview(one) }}</span>
                    <span v-if="one.unread" class="ft-row__badge" data-test="unread">{{ one.unread }}</span>
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
                    <span v-if="chat.unread" class="ft-row__badge" data-test="unread">{{ chat.unread }}</span>
                  </span>
                </span>
              </button>
              <!-- Issue app#1: this contact's own settings (name, history, burning). -->
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

          <!-- Open hidden sessions, one panel each under the main list: a chevron, a QR button and a
               leave button, no name. Closed ones leave no trace. -->
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
              <button
                type="button"
                class="ft-session__action"
                data-test="session-add"
                :aria-label="$t('session.addContact')"
                :title="$t('session.addContact')"
                @click="addTo(session)"
              >
                <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
              </button>
              <button
                v-if="session.chats.length"
                type="button"
                class="ft-session__action"
                data-test="session-new-circle"
                :aria-label="$t('circle.new')"
                :title="$t('circle.new')"
                @click="newCircle(session)"
              >
                <ion-icon :icon="peopleOutline" aria-hidden="true" />
              </button>
              <!-- A3: a session can go for good; the trash asks once by turning into a check. -->
              <button
                v-if="removing !== keyOf(session)"
                type="button"
                class="ft-session__action ft-session__action--leave"
                data-test="session-remove"
                :aria-label="$t('session.remove')"
                :title="$t('session.remove')"
                @click="removing = keyOf(session)"
              >
                <ion-icon :icon="trashOutline" aria-hidden="true" />
              </button>
              <button
                v-else
                type="button"
                class="ft-session__action ft-session__action--danger"
                data-test="session-remove-sure"
                :aria-label="$t('session.removeSure')"
                :title="$t('session.removeSure')"
                @click="remove(session)"
              >
                <ion-icon :icon="checkmarkOutline" aria-hidden="true" />
              </button>
              <button
                type="button"
                class="ft-session__action ft-session__action--leave"
                data-test="session-close"
                :aria-label="$t('session.close')"
                :title="$t('session.close')"
                @click="closeSession(session.id)"
              >
                <ion-icon :icon="logOutOutline" aria-hidden="true" />
              </button>
            </header>

            <template v-if="!isFolded(session.id)">
              <!-- A5: the session's own requests, from whoever scanned its QR. -->
              <ul v-if="session.requests.length" class="ft-rows ft-requests__rows" data-test="session-requests">
                <li v-for="chat in session.requests" :key="chat.id" data-test="request-row">
                  <button type="button" class="ft-row" @click="open(chat.id)">
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
                        <ion-icon :icon="peopleOutline" class="ft-row__kind" role="img" :aria-label="$t('circle.title')" />
                        <span class="ft-row__preview" dir="auto">{{ circlePreview(one) }}</span>
                        <span v-if="one.unread" class="ft-row__badge" data-test="unread">{{ one.unread }}</span>
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
                        <span v-if="chat.unread" class="ft-row__badge" data-test="unread">{{ chat.unread }}</span>
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
        </ion-content>
      </section>

      <section v-if="wide && selectedCircle" class="ft-chats__detail">
        <CircleThread :circle-id="selectedCircle" :active="onScreen" />
      </section>
      <section v-else-if="wide && selectedId" class="ft-chats__detail">
        <ChatThread :chat-id="selectedId" split :active="onScreen" />
      </section>
    </div>
  </ion-page>
</template>

<style scoped>
.ft-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-3);
  padding: 20vh var(--ft-space-5) 0;
  text-align: center;
}
.ft-empty__title {
  margin: 0;
  font-size: var(--ft-font-title);
  font-weight: 600;
}
.ft-empty__hint {
  margin: 0;
  max-width: 28ch;
  color: var(--ft-muted);
  line-height: 1.4;
}
.ft-empty__action {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 8px;
  margin-top: var(--ft-space-2);
  padding: 12px 20px;
  border: 0;
  border-radius: 999px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}

.ft-chats {
  display: flex;
  height: 100%;
}
.ft-chats__list {
  position: relative;
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
}
.ft-chats.is-wide .ft-chats__list {
  flex: 0 0 var(--ft-list-width);
  border-inline-end: 1px solid var(--ft-border);
}
.ft-chats__detail {
  position: relative;
  flex: 1;
  min-width: 0;
}

.ft-row {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
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
.ft-row:hover {
  background: color-mix(in srgb, var(--ft-surface-2) 60%, transparent);
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

/* A5: the requests, apart from the list and before it. */
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
.ft-session__action--danger {
  color: var(--ion-color-danger);
}

/* A hidden session's panel: a header that folds, then the same rows as the main list. */
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
.ft-session__action {
  appearance: none;
  display: grid;
  flex: none;
  place-items: center;
  width: 36px;
  height: 36px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--ft-muted);
  font-size: 18px;
  cursor: pointer;
}
.ft-session__action:hover {
  background: color-mix(in srgb, var(--ft-surface-2) 60%, transparent);
  color: var(--ft-text);
}
.ft-session__action--leave {
  color: var(--ft-accent);
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
</style>
