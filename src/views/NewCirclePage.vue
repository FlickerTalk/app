<script setup lang="ts">
import { computed, ref } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonIcon, IonPage, IonTitle, IonToolbar } from "@ionic/vue";
import { checkmarkOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import { createCircle, store } from "../core";

// A new circle (2026-09-27): a name and who is in it, from the contacts of the main list or of
// the open session it is made in. This phone is its admin.
const router = useRouter();
const route = useRoute();
const session = typeof route.query.session === "string" ? route.query.session : undefined;

const contacts = computed(() => {
  const list = session ? (store.sessions.find((candidate) => candidate.id === session)?.chats ?? []) : store.chats;
  return list.filter((chat) => !chat.blocked);
});
const name = ref("");
const chosen = ref(new Set<string>());
const busy = ref(false);
const error = ref("");

function toggle(id: string) {
  const next = new Set(chosen.value);
  if (next.has(id)) next.delete(id);
  else next.add(id);
  chosen.value = next;
}

const ready = computed(() => name.value.trim().length > 0 && chosen.value.size > 0 && !busy.value);

async function create() {
  if (!ready.value) return;
  busy.value = true;
  error.value = "";
  try {
    const id = await createCircle(name.value, [...chosen.value], session);
    router.replace(`/circle/${id}`);
  } catch (trouble) {
    error.value = String(trouble);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/chats" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("circle.new") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div class="ft-new">
        <p class="ft-new__hint">{{ $t("circle.newHint") }}</p>
        <input
          v-model="name"
          class="ft-new__name"
          data-test="circle-name"
          :maxlength="40"
          :aria-label="$t('circle.name')"
          :placeholder="$t('circle.name')"
        />
        <h2 class="ft-new__label">{{ $t("circle.pick") }}</h2>
        <p v-if="!contacts.length" class="ft-new__empty">{{ $t("chats.emptyHint") }}</p>
        <ul class="ft-new__list">
          <li v-for="chat in contacts" :key="chat.id">
            <button
              type="button"
              class="ft-new__row"
              :class="{ 'is-chosen': chosen.has(chat.id) }"
              role="checkbox"
              :aria-checked="chosen.has(chat.id)"
              :data-test="`pick-${chat.id}`"
              @click="toggle(chat.id)"
            >
              <Avatar :name="chat.name" :hue="chat.hue" :size="40" />
              <span class="ft-new__row-name">{{ chat.name }}</span>
              <span class="ft-new__check" aria-hidden="true">
                <ion-icon v-if="chosen.has(chat.id)" :icon="checkmarkOutline" />
              </span>
            </button>
          </li>
        </ul>
        <p v-if="error" class="ft-new__error" role="alert">{{ error }}</p>
        <button type="button" class="ft-new__create" data-test="circle-create" :disabled="!ready" @click="create">
          {{ $t("circle.create") }}
        </button>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-new {
  display: flex;
  flex-direction: column;
  gap: var(--ft-space-3);
  max-width: 560px;
  margin: 0 auto;
  padding: var(--ft-space-3) var(--ft-space-4) var(--ft-space-5);
}
.ft-new__hint,
.ft-new__empty {
  margin: 0;
  color: var(--ft-muted);
  font-size: 14px;
  line-height: 1.4;
}
.ft-new__name {
  width: 100%;
  min-height: 48px;
  padding: 0 16px;
  border: 1px solid var(--ft-border);
  border-radius: 14px;
  background: var(--ft-surface);
  color: var(--ft-text);
  font: inherit;
  font-size: 17px;
}
.ft-new__label {
  margin: var(--ft-space-2) 0 0;
  font-size: 13px;
  font-weight: 600;
  color: var(--ft-accent);
}
.ft-new__list {
  margin: 0;
  padding: 0;
  list-style: none;
}
.ft-new__row {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 12px;
  width: 100%;
  padding: 8px 10px;
  border: 0;
  border-radius: 14px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: start;
  cursor: pointer;
}
.ft-new__row.is-chosen {
  background: color-mix(in srgb, var(--ft-accent) 12%, transparent);
}
.ft-new__row-name {
  flex: 1;
  font-size: 16px;
  font-weight: 500;
}
.ft-new__check {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  border: 2px solid var(--ft-border);
  border-radius: 50%;
  color: var(--ft-on-accent);
  font-size: 16px;
}
.is-chosen .ft-new__check {
  border-color: var(--ft-accent);
  background: var(--ft-accent);
}
.ft-new__error {
  margin: 0;
  color: var(--ion-color-danger);
  font-size: 13px;
}
.ft-new__create {
  appearance: none;
  margin-top: var(--ft-space-2);
  padding: 14px 20px;
  border: 0;
  border-radius: 999px;
  background: linear-gradient(135deg, var(--ft-accent), var(--ft-accent-2));
  color: var(--ft-on-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-new__create:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
