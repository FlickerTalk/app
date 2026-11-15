<script setup lang="ts">
import { ref } from "vue";
import { IonIcon } from "@ionic/vue";
import { checkmarkOutline, logOutOutline, peopleOutline, qrCodeOutline, trashOutline } from "ionicons/icons";
import { useRouter } from "vue-router";
import { closeSession, removeSession, type Session } from "../core";

const props = defineProps<{ session: Session }>();

const router = useRouter();
const removing = ref(false);

function addContact() {
  if (props.session.id) router.push(`/add-contact?session=${props.session.id}`);
}

function newCircle() {
  if (props.session.id) router.push(`/new-circle?session=${props.session.id}`);
}

async function remove() {
  removing.value = false;
  if (props.session.id) await removeSession(props.session.id);
  else await closeSession("");
}
</script>

<template>
  <button
    type="button"
    class="ft-session__action"
    data-test="session-add"
    :aria-label="$t('session.addContact')"
    :title="$t('session.addContact')"
    @click="addContact"
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
    @click="newCircle"
  >
    <ion-icon :icon="peopleOutline" aria-hidden="true" />
  </button>
  <button
    v-if="!removing"
    type="button"
    class="ft-session__action ft-session__action--leave"
    data-test="session-remove"
    :aria-label="$t('session.remove')"
    :title="$t('session.remove')"
    @click="removing = true"
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
    @click="remove"
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
</template>

<style scoped>
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
.ft-session__action--danger {
  color: var(--ion-color-danger);
}
</style>
