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
import {
  checkmarkCircleOutline,
  cloudOutline,
  cloudUploadOutline,
  copyOutline,
  keyOutline,
  lockClosedOutline,
  shareSocialOutline,
  sparklesOutline,
  timeOutline,
  warningOutline,
} from "ionicons/icons";
import {
  formatSize,
  PHRASE_MAX,
  PHRASE_MIN,
  shareText,
  vaultBackup,
  vaultChangePhrase,
  vaultConnect,
  vaultDisconnect,
  vaultRestore,
  vaultSetup,
  vaultStatus,
  vaultSuggestPhrase,
  vaultUnlock,
  VAULT_EVENT,
  VAULT_PROGRESS_EVENT,
  type VaultStatus,
} from "../core";
import { t } from "../i18n";

// Plan-drive (2026-09-27), §61: a sealed copy of this phone in the user's own cloud. The core
// logs in through the system browser and seals everything on the phone; this page only asks and
// shows. Nothing of it reaches our server. The recovery phrase (plan-recuperacion, 2026-09-28)
// is the user's: written here twice, kept wherever they like, never by the app. It is only ever
// typed on this page, never in a plugin; whoever does not set it up cannot recover the account.
const status = ref<VaultStatus>({ state: "none", provider: null, drive: null, problem: null });
/** The phrase form, when open: for a new drive, or to change the phrase of this one. */
const form = ref<"setup" | "change" | null>(null);
const phrase = ref("");
const again = ref("");
const typed = ref("");
const working = ref(false);
const error = ref("");
const progress = ref<{ done: number; total: number } | null>(null);
const asksToRestore = ref(false);
const asksToForget = ref(false);
const restarting = ref(false);
const done = ref("");

/** A phrase as the core counts it: trimmed, composed, in characters. */
const counted = (value: string) => value.trim().normalize("NFC");
const length = computed(() => [...counted(phrase.value)].length);
const mismatch = computed(() => again.value !== "" && counted(again.value) !== counted(phrase.value));
const acceptable = computed(() => length.value >= PHRASE_MIN && length.value <= PHRASE_MAX && !mismatch.value && again.value !== "");
/** What went wrong, honestly (§84); a wrong phrase is just that, not "something went wrong". */
const shownError = computed(() => {
  const said = error.value || status.value.problem || "";
  return said === t("backup.wrongPhrase") ? said : t("backup.failed", { error: said });
});
const retryAt = computed(() => (status.value.retryAt ? new Date(status.value.retryAt).toLocaleString() : ""));

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
    const said = String(failure);
    error.value = said.includes("does not open this drive") ? t("backup.wrongPhrase") : said;
  } finally {
    working.value = false;
    progress.value = null;
  }
  await refresh();
}

const connect = () => step(async () => void (await vaultConnect("google")));
function openForm(kind: "setup" | "change") {
  form.value = kind;
  phrase.value = "";
  again.value = "";
  error.value = "";
  done.value = "";
}

function closeForm() {
  form.value = null;
  phrase.value = "";
  again.value = "";
}

async function suggest() {
  const suggested = await vaultSuggestPhrase().catch(() => "");
  phrase.value = suggested;
  again.value = suggested;
}

/** To keep it wherever the user likes: a password manager, a note, another account. */
async function copyPhrase() {
  await navigator.clipboard?.writeText(phrase.value).catch(() => undefined);
}

async function sharePhrase() {
  await shareText(phrase.value).catch(() => undefined);
}

const savePhrase = () =>
  step(async () => {
    const kind = form.value;
    if (kind === "setup") await vaultSetup(phrase.value);
    else await vaultChangePhrase(phrase.value);
    closeForm();
    if (kind === "change") done.value = t("backup.phraseChanged");
  });
const unlock = () =>
  step(async () => {
    try {
      await vaultUnlock(typed.value);
    } finally {
      typed.value = "";
    }
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
        <ion-title><span class="ft-title">{{ $t("backup.title") }}</span></ion-title>
      </ion-toolbar>
    </ion-header>

    <ion-content class="ion-padding">
      <div class="ft-backup">
        <template v-if="restarting">
          <ion-icon :icon="checkmarkCircleOutline" class="ft-backup__big is-done" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.restored") }}</p>
          <p class="ft-backup__hint">{{ $t("move.restarting") }}</p>
        </template>

        <!-- The recovery phrase: the user's, written twice; the app forgets it once it is used. -->
        <template v-else-if="form">
          <ion-icon :icon="keyOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t(form === "setup" ? "backup.phraseTitle" : "backup.changePhrase") }}</p>
          <p class="ft-backup__hint">{{ $t("backup.phraseHint") }}</p>
          <input
            v-model="phrase"
            type="text"
            class="ft-backup__input ft-backup__phrase"
            autocomplete="off"
            autocapitalize="off"
            autocorrect="off"
            spellcheck="false"
            :maxlength="PHRASE_MAX * 2"
            :placeholder="$t('backup.phrasePlaceholder')"
            :aria-label="$t('backup.phrasePlaceholder')"
            data-test="phrase"
          />
          <input
            v-model="again"
            type="text"
            class="ft-backup__input ft-backup__phrase"
            autocomplete="off"
            autocapitalize="off"
            autocorrect="off"
            spellcheck="false"
            :maxlength="PHRASE_MAX * 2"
            :placeholder="$t('backup.phraseAgain')"
            :aria-label="$t('backup.phraseAgain')"
            data-test="phrase-again"
          />
          <p class="ft-backup__hint" data-test="phrase-rule">{{ $t("backup.phraseRule", { min: PHRASE_MIN, max: PHRASE_MAX }) }} · {{ length }}</p>
          <p v-if="mismatch" class="ft-backup__error" data-test="phrase-mismatch">{{ $t("backup.phraseMismatch") }}</p>
          <div class="ft-backup__tools">
            <button type="button" class="ft-backup__tool" data-test="suggest" :aria-label="$t('backup.suggest')" @click="suggest">
              <ion-icon :icon="sparklesOutline" aria-hidden="true" /> {{ $t("backup.suggest") }}
            </button>
            <button type="button" class="ft-backup__tool" data-test="copy-phrase" :disabled="!phrase" :aria-label="$t('backup.copy')" @click="copyPhrase">
              <ion-icon :icon="copyOutline" aria-hidden="true" /> {{ $t("backup.copy") }}
            </button>
            <button type="button" class="ft-backup__tool" data-test="share-phrase" :disabled="!phrase" :aria-label="$t('backup.share')" @click="sharePhrase">
              <ion-icon :icon="shareSocialOutline" aria-hidden="true" /> {{ $t("backup.share") }}
            </button>
          </div>
          <p class="ft-backup__hint ft-backup__warn">{{ $t("backup.notSameGoogle") }}</p>
          <ion-button shape="round" data-test="phrase-confirm" :disabled="working || !acceptable" @click="savePhrase">
            {{ $t("backup.phraseConfirm") }}
          </ion-button>
          <ion-button fill="clear" size="small" :disabled="working" @click="closeForm">{{ $t("common.cancel") }}</ion-button>
        </template>

        <template v-else-if="status.state === 'none'">
          <ion-icon :icon="cloudOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.none") }}</p>
          <p class="ft-backup__hint">{{ $t("backup.noneHint") }}</p>
          <ion-button shape="round" data-test="connect" :disabled="working" @click="connect">
            {{ $t("backup.connectGoogle") }}
          </ion-button>
        </template>

        <template v-else-if="status.state === 'empty' || status.state === 'outdated'">
          <ion-icon :icon="cloudUploadOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t(status.state === "empty" ? "backup.empty" : "backup.outdated") }}</p>
          <p class="ft-backup__hint">{{ $t(status.state === "empty" ? "backup.emptyHint" : "backup.outdatedHint") }}</p>
          <ion-button shape="round" data-test="set-up" :disabled="working" @click="openForm('setup')">{{ $t("backup.setUp") }}</ion-button>
        </template>

        <template v-else-if="status.state === 'locked'">
          <ion-icon :icon="retryAt ? timeOutline : lockClosedOutline" class="ft-backup__big" aria-hidden="true" />
          <p class="ft-backup__title">{{ $t("backup.locked") }}</p>
          <!-- Too many wrong phrases on this phone: not even the right one is tried until then. -->
          <p v-if="retryAt" class="ft-backup__hint" role="status" data-test="retry-at">{{ $t("backup.retryAt", { when: retryAt }) }}</p>
          <template v-else>
            <p class="ft-backup__hint">{{ $t("backup.unlockHint") }}</p>
            <div class="ft-backup__row">
              <input
                v-model="typed"
                type="text"
                class="ft-backup__input"
                autocomplete="off"
                autocapitalize="off"
                autocorrect="off"
                spellcheck="false"
                :placeholder="$t('backup.phrasePlaceholder')"
                :aria-label="$t('backup.phrasePlaceholder')"
                data-test="phrase-input"
              />
              <button type="button" class="ft-backup__go" data-test="unlock" :disabled="working || !typed.trim()" @click="unlock">
                {{ $t("backup.unlockNow") }}
              </button>
            </div>
            <p v-if="(status.triesLeft ?? 5) < 5" class="ft-backup__hint" data-test="tries-left">
              {{ $t("backup.triesLeft", { count: status.triesLeft ?? 0 }) }}
            </p>
          </template>
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

          <ion-button fill="clear" size="small" data-test="change-phrase" :disabled="working" @click="openForm('change')">
            {{ $t("backup.changePhrase") }}
          </ion-button>
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
          <ion-icon :icon="warningOutline" aria-hidden="true" /> {{ shownError }}
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
.ft-backup__phrase {
  flex: none;
  width: 100%;
  box-sizing: border-box;
}
.ft-backup__tools {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 8px;
}
.ft-backup__tool {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  border: 1px solid var(--ft-border);
  border-radius: 999px;
  color: var(--ft-text);
  background: var(--ft-surface);
}
.ft-backup__tool:disabled {
  opacity: 0.5;
}
.ft-backup__warn {
  font-weight: 600;
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
  color: var(--ft-on-accent);
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
