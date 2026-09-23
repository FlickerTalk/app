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
} from "@ionic/vue";
import {
  attachOutline,
  checkmark,
  checkmarkDone,
  chevronForwardOutline,
  ellipsisVerticalOutline,
  logOutOutline,
  micOutline,
  qrCodeOutline,
  timeOutline,
} from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import ChatThread from "../components/ChatThread.vue";
import { closeSession, store } from "../core";

const router = useRouter();

const wideQuery = window.matchMedia("(min-width: 768px)");
const wide = ref(wideQuery.matches);
const onWidthChange = (event: MediaQueryListEvent) => {
  wide.value = event.matches;
};
onMounted(() => wideQuery.addEventListener("change", onWidthChange));
onBeforeUnmount(() => wideQuery.removeEventListener("change", onWidthChange));

const selectedId = ref(store.chats[0]?.id ?? "");

function open(id: string) {
  if (wide.value) {
    selectedId.value = id;
  } else {
    router.push(`/chat/${id}`);
  }
}

// Hidden sessions fold like the panels of a sidebar; each remembers whether it is open.
const folded = ref<Record<string, boolean>>({});
const isFolded = (id: string) => folded.value[id] === true;
function fold(id: string) {
  folded.value = { ...folded.value, [id]: !isFolded(id) };
}

const STATUS_ICON: Record<string, string> = {
  pending: timeOutline,
  sent: checkmark,
  delivered: checkmarkDone,
  read: checkmarkDone,
};
</script>

<template>
  <ion-page>
    <div class="ft-chats" :class="{ 'is-wide': wide }">
      <section class="ft-chats__list">
        <ion-header class="ion-no-border">
          <ion-toolbar>
            <ion-title>{{ $t("tabs.chats") }}</ion-title>
            <ion-buttons slot="end">
              <ion-button :aria-label="$t('addContact.title')" @click="router.push('/add-contact')">
                <ion-icon slot="icon-only" :icon="qrCodeOutline" aria-hidden="true" />
              </ion-button>
            </ion-buttons>
          </ion-toolbar>
        </ion-header>

        <ion-content>
          <ion-header collapse="condense" class="ion-no-border">
            <ion-toolbar>
              <ion-title size="large">{{ $t("tabs.chats") }}</ion-title>
            </ion-toolbar>
          </ion-header>

          <div v-if="!store.chats.length" class="ft-empty" data-test="empty">
            <p class="ft-empty__title">{{ $t("chats.empty") }}</p>
            <p class="ft-empty__hint">{{ $t("chats.emptyHint") }}</p>
            <button type="button" class="ft-empty__action" @click="router.push('/add-contact')">
              <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
              {{ $t("addContact.title") }}
            </button>
          </div>

          <ul v-else class="ft-rows">
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
                    <span class="ft-row__name">{{ chat.name }}</span>
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
                    <span class="ft-row__preview">{{ chat.lastKind === "voice" ? $t("chat.voiceMessage") : chat.preview }}</span>
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
            :key="session.id"
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
                @click="router.push(`/add-contact?session=${session.id}`)"
              >
                <ion-icon :icon="qrCodeOutline" aria-hidden="true" />
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
              <p v-if="!session.chats.length" class="ft-session__empty">{{ $t("session.empty") }}</p>
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
                        <span class="ft-row__name">{{ chat.name }}</span>
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
                        <span class="ft-row__preview">{{ chat.lastKind === "voice" ? $t("chat.voiceMessage") : chat.preview }}</span>
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

      <section v-if="wide && selectedId" class="ft-chats__detail">
        <ChatThread :chat-id="selectedId" />
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
  flex: 0 0 360px;
  border-right: 1px solid var(--ft-border);
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
  text-align: left;
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
  text-align: left;
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
