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
import { checkmark, checkmarkDone, qrCodeOutline, timeOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import ChatThread from "../components/ChatThread.vue";
import data from "../mock/chats.json";

const router = useRouter();

const wideQuery = window.matchMedia("(min-width: 768px)");
const wide = ref(wideQuery.matches);
const onWidthChange = (event: MediaQueryListEvent) => {
  wide.value = event.matches;
};
onMounted(() => wideQuery.addEventListener("change", onWidthChange));
onBeforeUnmount(() => wideQuery.removeEventListener("change", onWidthChange));

const selectedId = ref(data.chats[0].id);

function open(id: string) {
  if (wide.value) {
    selectedId.value = id;
  } else {
    router.push(`/chat/${id}`);
  }
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
            <ion-title>Chats</ion-title>
            <ion-buttons slot="end">
              <ion-button aria-label="Add contact">
                <ion-icon slot="icon-only" :icon="qrCodeOutline" aria-hidden="true" />
              </ion-button>
            </ion-buttons>
          </ion-toolbar>
        </ion-header>

        <ion-content>
          <ion-header collapse="condense" class="ion-no-border">
            <ion-toolbar>
              <ion-title size="large">Chats</ion-title>
            </ion-toolbar>
          </ion-header>

          <ul class="ft-rows">
            <li v-for="chat in data.chats" :key="chat.id">
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
                    <span class="ft-row__preview">{{ chat.preview }}</span>
                    <span v-if="chat.unread" class="ft-row__badge" data-test="unread">{{ chat.unread }}</span>
                  </span>
                </span>
              </button>
            </li>
          </ul>
        </ion-content>
      </section>

      <section v-if="wide" class="ft-chats__detail">
        <ChatThread :chat-id="selectedId" />
      </section>
    </div>
  </ion-page>
</template>

<style scoped>
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
.ft-row__preview {
  flex: 1;
  font-size: 14px;
  color: var(--ft-muted);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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
