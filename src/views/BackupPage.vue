<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  IonBackButton,
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonIcon,
  IonPage,
  IonTitle,
  IonToolbar,
} from "@ionic/vue";
import { checkmarkCircleOutline, cloudOutline, cloudUploadOutline, keyOutline, lockClosedOutline, warningOutline } from "ionicons/icons";
import {
  formatSize,
  vaultBackup,
  vaultConnect,
  vaultDisconnect,
  vaultRestore,
  vaultSetup,
  vaultStatus,
  vaultUnlock,
  VAULT_EVENT,
  VAULT_PROGRESS_EVENT,
  type VaultStatus,
} from "../core";
import { t } from "../i18n";

// Plan-drive (2026-09-27), §61: a sealed copy of this phone in the user's own cloud. The core
// logs in through the system browser, seals everything on the phone and shows the recovery
// code once; this page only asks and shows. Nothing of it reaches our server.
const status = ref<VaultStatus>({ state: "none", provider: null, drive: null, problem: null });
const code = ref("");
const typed = ref("");
const working = ref(false);
const error = ref("");
const progress = ref<{ done: number; total: number } | null>(null);
const asksToRestore = ref(false);
const asksToForget = ref(false);
const restarting = ref(false);
const done = ref("");

const percent = computed(() => (progress.value?.total ? Math.round((progress.value.done / progress.value.total) * 100) : 0));
const lastBackup = computed(() => {
  const at = status.value.drive?.backupAt;
  return at ? new Date(at).toLocaleString() : "";
});

let stopListening: Array<() => void> = [];

onMounted(async () => {
  await refresh();
  stopListening = await Promise.all([
    listen(VAULT_EVENT, () => void refresh()).catch(() => () => undefined),
    listen<{ done: number; total: number }>(VAULT_PROGRESS_EVENT, ({ payload }) => {
      progress.value = payload;
    }).catch(() => () => undefined),
  ]);
});
onBeforeUnmount(() => {
  for (const stop of stopListening) {
    void Promise.resolve()
      .then(() => stop())
      .catch(() => undefined);
  }
});

async function refresh() {
  status.value = await vaultStatus().catch(() => status.value);
}

/** Runs one step against the core, showing that it works and, honestly, when it did not (§84). */
async function step(work: () => Promise<void>) {
  working.value = true;
  error.value = "";
  done.value = "";
  progress.value = null;
  try {
    await work();
  } catch (failure) {
    error.value = String(failure);
  } finally {
    working.value = false;
    progress.value = null;
  }
  await refresh();
}

const connect = () => step(async () => void (await vaultConnect("google")));
const setUp = () =>
  step(async () => {
    code.value = await vaultSetup();
  });
const unlock = () =>
  step(async () => {
    await vaultUnlock(typed.value);
    typed.value = "";
  });
const backUp = () =>
  step(async () => {
    await vaultBackup();
    done.value = t("backup.done");
  });
const restore = () =>
  step(async () => {
    asksToRestore.value = false;
    await vaultRestore();
    restarting.value = true;
  });
const forget = () =>
  step(async () => {
    asksToForget.value = false;
    await vaultDisconnect();
  });
</script>

<template>
  <ion-page>
    <ion-header class="ion-no-border">
      <ion-toolbar>
        <ion-buttons slot="start">
          <ion-back-button default-href="/tabs/settings" :aria-label="$t('common.back')" />
        </ion-buttons>
        <ion-title>{{ $t("backup.title") }}</ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ion-padding">
      <div class="ft-backup">
        <template v-if="restarting">
          <ion-icon :icon="checkmarkCircleOutline" class="ft-backup__big is-done" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.restored") }}</p>
          <p class="ft-backup__hint">{{ $t("move.restarting") }}</p>
        </template>

        <!-- The recovery code, once: the app forgets it the moment this page does. -->
        <template v-else-if="code">
          <ion-icon :icon="keyOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.codeTitle") }}</p>
          <code class="ft-backup__code" data-test="recovery-code">{{ code }}</code>
          <p class="ft-backup__hint">{{ $t("backup.codeHint") }}</p>
          <ion-button shape="round" data-test="code-done" @click="code = ''">{{ $t("backup.codeDone") }}</ion-button>
        </template>

        <template v-else-if="status.state === 'none'">
          <ion-icon :icon="cloudOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.none") }}</p>
          <p class="ft-backup__hint">{{ $t("backup.noneHint") }}</p>
          <ion-button shape="round" data-test="connect" :disabled="working" @click="connect">
            {{ $t("backup.connectGoogle") }}
          </ion-button>
        </template>

        <template v-else-if="status.state === 'empty'">
          <ion-icon :icon="cloudUploadOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.empty") }}</p>
          <p class="ft-backup__hint">{{ $t("backup.emptyHint") }}</p>
          <ion-button shape="round" data-test="set-up" :disabled="working" @click="setUp">{{ $t("backup.setUp") }}</ion-button>
        </template>

        <template v-else-if="status.state === 'locked'">
          <ion-icon :icon="lockClosedOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.locked") }}</p>
          <p class="ft-backup__hint">{{ $t("backup.unlockHint") }}</p>
          <div class="ft-backup__row">
            <input
              v-model="typed"
              type="text"
              class="ft-backup__input"
              autocapitalize="characters"
              autocomplete="off"
              :placeholder="$t('backup.codePlaceholder')"
              :aria-label="$t('backup.codePlaceholder')"
              data-test="code-input"
            />
            <button type="button" class="ft-backup__go" data-test="unlock" :disabled="working || !typed.trim()" @click="unlock">
              {{ $t("backup.unlockNow") }}
            </button>
          </div>
        </template>

        <template v-else>
          <ion-icon :icon="cloudUploadOutline" class="ft-backup__big is-done" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.ready") }}</p>
          <p class="ft-backup__hint" data-test="last-backup">
            {{ lastBackup ? $t("backup.lastBackup", { when: lastBackup }) : $t("backup.noBackup") }}
          </p>
          <p v-if="status.drive" class="ft-backup__hint" data-test="usage">
            {{ $t("backup.used", { size: formatSize(status.drive.used) }) }}
            <template v-if="status.drive.pending"> · {{ $t("backup.pending", { count: status.drive.pending }) }}</template>
          </p>
          <ion-button shape="round" data-test="back-up" :disabled="working" @click="backUp">{{ $t("backup.backupNow") }}</ion-button>

          <template v-if="status.drive?.backupAt">
            <ion-button v-if="!asksToRestore" fill="outline" shape="round" data-test="restore" :disabled="working" @click="asksToRestore = true">
              {{ $t("backup.restore") }}
            </ion-button>
            <div v-else class="ft-backup__ask" data-test="restore-ask">
              <p class="ft-backup__hint">{{ $t("backup.restoreAsk") }}</p>
              <ion-button size="small" fill="clear" @click="asksToRestore = false">{{ $t("common.cancel") }}</ion-button>
              <ion-button size="small" color="danger" data-test="restore-confirm" @click="restore">{{ $t("backup.restoreConfirm") }}</ion-button>
            </div>
          </template>

          <ion-button v-if="!asksToForget" fill="clear" size="small" data-test="forget" :disabled="working" @click="asksToForget = true">
            {{ $t("backup.forget") }}
          </ion-button>
          <div v-else class="ft-backup__ask" data-test="forget-ask">
            <p class="ft-backup__hint">{{ $t("backup.forgetHint") }}</p>
            <ion-button size="small" fill="clear" @click="asksToForget = false">{{ $t("common.cancel") }}</ion-button>
            <ion-button size="small" color="danger" data-test="forget-confirm" @click="forget">{{ $t("backup.forgetConfirm") }}</ion-button>
          </div>
        </template>

        <span
          v-if="working"
          class="ft-backup__progress"
          role="progressbar"
          aria-valuemin="0"
          aria-valuemax="100"
          :aria-valuenow="percent"
          :aria-label="$t('backup.working')"
        >
          <span class="ft-backup__bar" :style="{ width: `${progress ? percent : 100}%` }" />
        </span>
        <p v-if="done" class="ft-backup__hint is-done" role="status">{{ done }}</p>
        <p v-if="error || status.problem" class="ft-backup__error" role="alert">
          <ion-icon :icon="warningOutline" aria-hidden="true" /> {{ $t("backup.failed", { error: error || status.problem }) }}
        </p>
        <p class="ft-backup__hint ft-backup__foot">{{ $t("backup.sees") }}</p>
      </div>
    </ion-content>
  </ion-page>
</template>

<style scoped>
.ft-backup {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: var(--ft-space-4);
  max-width: 460px;
  margin: 6vh auto 0;
  text-align: center;
}
.ft-backup__big {
  font-size: 64px;
  color: var(--ft-accent);
}
.ft-backup__big.is-done {
  color: var(--ion-color-success);
}
.ft-backup__title {
  margin: 0;
  font-size: 20px;
  font-weight: 700;
}
.ft-backup__hint {
  margin: 0;
  color: var(--ft-muted);
  font-size: 14px;
}
.ft-backup__hint.is-done {
  color: var(--ion-color-success);
}
.ft-backup__foot {
  margin-top: var(--ft-space-4);
  font-size: 12px;
}
.ft-backup__code {
  padding: 12px 14px;
  border-radius: 14px;
  background: var(--ft-surface-2);
  font-size: 18px;
  letter-spacing: 1px;
  user-select: all;
  word-break: break-all;
}
.ft-backup__row {
  display: flex;
  gap: 8px;
  width: 100%;
}
.ft-backup__input {
  flex: 1;
  min-width: 0;
  padding: 12px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 14px;
  color: var(--ft-text);
  background: var(--ft-surface);
  font-family: monospace;
}
.ft-backup__go {
  padding: 0 16px;
  border: 0;
  border-radius: 14px;
  font-weight: 600;
  color: #fff;
  background: var(--ft-accent);
}
.ft-backup__go:disabled {
  opacity: 0.5;
}
.ft-backup__ask {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  align-items: center;
  gap: 4px;
}
.ft-backup__progress {
  display: block;
  width: 100%;
  height: 6px;
  border-radius: 6px;
  overflow: hidden;
  background: var(--ft-surface-2);
}
.ft-backup__bar {
  display: block;
  height: 100%;
  background: var(--ft-accent);
  transition: width 0.3s ease;
}
.ft-backup__error {
  margin: 0;
  color: var(--ion-color-danger);
  font-size: 14px;
}
</style>
