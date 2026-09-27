<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { IonBackButton, IonButtons, IonContent, IonHeader, IonIcon, IonItem, IonLabel, IonList, IonPage, IonToggle, IonToolbar } from "@ionic/vue";
import { checkmarkOutline, logOutOutline, pencilOutline, personAddOutline, personRemoveOutline, shieldOutline, trashOutline } from "ionicons/icons";
import { useRoute, useRouter } from "vue-router";
import Avatar from "../components/Avatar.vue";
import {
  circle as circleOf,
  circleHome,
  forgetCircle,
  hueOf,
  inviteToCircle,
  leaveCircle,
  removeFromCircle,
  renameCircle,
  setCircleAdmin,
  setCircleAdminsOnly,
} from "../core";

// A circle's settings (2026-09-27): who is in it, who runs it, and what this phone may do about
// it. Only an admin changes anything; everyone can leave, and a circle one is no longer in can
// be deleted here.
const route = useRoute();
const router = useRouter();
const id = String(route.params.id);
const circle = computed(() => circleOf(id));
const admin = computed(() => circle.value?.admin ?? false);

const name = ref(circle.value?.name ?? "");
watch(
  () => circle.value?.name,
  (fresh) => {
    if (fresh !== undefined) name.value = fresh;
  },
);

async function saveName() {
  if (name.value.trim() && name.value.trim() !== circle.value?.name) await renameCircle(id, name.value);
}

/** The contacts of the same list who are not in the circle yet. */
const candidates = computed(() => {
  const members = new Set(circle.value?.members.map((member) => member.id) ?? []);
  return circleHome(id).contacts.filter((chat) => !chat.blocked && !members.has(chat.id));
});
const inviting = ref(false);

async function invite(contact: string) {
  inviting.value = false;
  await inviteToCircle(id, contact);
}

// Taking someone out, leaving and deleting are for good: each asks once, in place.
const sure = ref("");

async function remove(contact: string) {
  if (sure.value !== `remove:${contact}`) {
    sure.value = `remove:${contact}`;
    return;
  }
  sure.value = "";
  await removeFromCircle(id, contact);
}

async function leave() {
  if (sure.value !== "leave") {
    sure.value = "leave";
    return;
  }
  sure.value = "";
  await leaveCircle(id);
}

async function forget() {
  if (sure.value !== "forget") {
    sure.value = "forget";
    return;
  }
  sure.value = "";
  await forgetCircle(id);
  router.replace("/tabs/chats");
}

async function toggleAdmin(contact: string, isAdmin: boolean) {
  await setCircleAdmin(id, contact, !isAdmin);
}
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button :default-href="`/circle/${id}`" :aria-label="$t('common.back')" />
        </ion-buttons>
      </ion-toolbar>
    </ion-header>

    <ion-content>
      <div v-if="circle" class="ft-circle">
        <Avatar :name="circle.name" :hue="circle.hue" :size="88" />
        <h1 class="ft-circle__name" data-test="circle-title">{{ circle.name }}</h1>
        <span class="ft-circle__status">{{ $t("circle.members", { count: circle.members.length }) }}</span>
        <p v-if="circle.left" class="ft-circle__left" data-test="circle-left">{{ $t("circle.left") }}</p>

        <ion-list v-if="admin" inset class="ft-group">
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="pencilOutline" aria-hidden="true" /></span>
            <input
              v-model="name"
              class="ft-circle__input"
              data-test="circle-rename"
              :maxlength="40"
              :aria-label="$t('circle.name')"
              :placeholder="$t('circle.name')"
              @keyup.enter="saveName"
            />
            <button
              slot="end"
              type="button"
              class="ft-circle__save"
              data-test="circle-save-name"
              :disabled="!name.trim() || name.trim() === circle.name"
              @click="saveName"
            >
              {{ $t("common.save") }}
            </button>
          </ion-item>
          <ion-item lines="none">
            <span slot="start" class="ft-tile"><ion-icon :icon="shieldOutline" aria-hidden="true" /></span>
            <ion-toggle
              :checked="circle.adminsOnly"
              data-test="circle-admins-only"
              :aria-label="$t('circle.adminsOnly')"
              @ion-change="setCircleAdminsOnly(id, $event.detail.checked)"
            >
              {{ $t("circle.adminsOnly") }}
              <p class="ft-circle__note">{{ $t("circle.adminsOnlyHint") }}</p>
            </ion-toggle>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item v-for="member in circle.members" :key="member.id" lines="none" data-test="circle-member">
            <Avatar slot="start" :name="member.name" :hue="hueOf(member.id)" :size="36" />
            <ion-label>
              <span class="ft-circle__member">{{ member.me ? $t("circle.you") : member.name }}</span>
              <span v-if="member.admin" class="ft-circle__badge" data-test="circle-admin-badge">{{ $t("circle.admin") }}</span>
            </ion-label>
            <template v-if="admin && !member.me">
              <button
                slot="end"
                type="button"
                class="ft-circle__action"
                :class="{ 'is-on': member.admin }"
                data-test="circle-toggle-admin"
                :aria-label="member.admin ? $t('circle.dropAdmin') : $t('circle.makeAdmin')"
                :title="member.admin ? $t('circle.dropAdmin') : $t('circle.makeAdmin')"
                @click="toggleAdmin(member.id, member.admin)"
              >
                <ion-icon :icon="shieldOutline" aria-hidden="true" />
              </button>
              <button
                slot="end"
                type="button"
                class="ft-circle__action ft-circle__action--danger"
                :data-test="sure === `remove:${member.id}` ? 'circle-remove-sure' : 'circle-remove'"
                :aria-label="$t('circle.remove')"
                :title="$t('circle.remove')"
                @click="remove(member.id)"
              >
                <ion-icon :icon="sure === `remove:${member.id}` ? checkmarkOutline : personRemoveOutline" aria-hidden="true" />
              </button>
            </template>
          </ion-item>
          <ion-item v-if="admin" button lines="none" data-test="circle-invite" @click="inviting = !inviting">
            <span slot="start" class="ft-tile"><ion-icon :icon="personAddOutline" aria-hidden="true" /></span>
            <ion-label>{{ $t("circle.invite") }}</ion-label>
          </ion-item>
        </ion-list>

        <ion-list v-if="inviting" inset class="ft-group" data-test="circle-candidates">
          <ion-item v-if="!candidates.length" lines="none">
            <ion-label class="ft-circle__note">{{ $t("circle.noOneToInvite") }}</ion-label>
          </ion-item>
          <ion-item v-for="chat in candidates" :key="chat.id" button lines="none" :data-test="`invite-${chat.id}`" @click="invite(chat.id)">
            <Avatar slot="start" :name="chat.name" :hue="chat.hue" :size="36" />
            <ion-label>{{ chat.name }}</ion-label>
          </ion-item>
        </ion-list>

        <ion-list inset class="ft-group">
          <ion-item v-if="!circle.left" button lines="none" :data-test="sure === 'leave' ? 'circle-leave-sure' : 'circle-leave'" @click="leave">
            <span slot="start" class="ft-tile ft-tile--danger"><ion-icon :icon="logOutOutline" aria-hidden="true" /></span>
            <ion-label color="danger">{{ sure === "leave" ? $t("circle.leaveSure") : $t("circle.leave") }}</ion-label>
          </ion-item>
          <ion-item v-else button lines="none" :data-test="sure === 'forget' ? 'circle-forget-sure' : 'circle-forget'" @click="forget">
            <span slot="start" class="ft-tile ft-tile--danger"><ion-icon :icon="trashOutline" aria-hidden="true" /></span>
            <ion-label color="danger">{{ sure === "forget" ? $t("circle.forgetSure") : $t("circle.forget") }}</ion-label>
          </ion-item>
        </ion-list>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-circle {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 6px;
  max-width: 560px;
  margin: 0 auto;
  padding: var(--ft-space-2) 0 var(--ft-space-5);
  text-align: center;
}
.ft-circle__name {
  margin: var(--ft-space-3) 0 0;
  font-size: 24px;
  font-weight: 700;
}
.ft-circle__status,
.ft-circle__note {
  margin: 0;
  font-size: var(--ft-font-meta);
  color: var(--ft-muted);
}
.ft-circle__left {
  margin: 4px 0 0;
  font-size: 13px;
  color: var(--ion-color-danger);
}
.ft-group {
  width: 100%;
  --ion-item-background: var(--ft-surface);
  border: 1px solid var(--ft-border);
  text-align: start;
}
.ft-tile--danger {
  background: color-mix(in srgb, var(--ion-color-danger) 16%, transparent);
  color: var(--ion-color-danger);
}
.ft-circle__input {
  flex: 1;
  min-width: 0;
  border: 0;
  background: transparent;
  color: var(--ft-text);
  font: inherit;
  font-size: 16px;
}
.ft-circle__save {
  appearance: none;
  border: 0;
  background: transparent;
  color: var(--ft-accent);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.ft-circle__save:disabled {
  opacity: 0.4;
}
.ft-circle__member {
  font-size: 16px;
}
.ft-circle__badge {
  margin-inline-start: 8px;
  padding: 2px 8px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--ft-accent) 16%, transparent);
  color: var(--ft-accent);
  font-size: 11px;
  font-weight: 600;
}
.ft-circle__action {
  appearance: none;
  display: grid;
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
.ft-circle__action.is-on {
  color: var(--ft-accent);
}
.ft-circle__action--danger {
  color: var(--ion-color-danger);
}
</style>
