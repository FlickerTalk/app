<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import { toastController } from "@ionic/vue";
import {
  pickForPlugin,
  takePhotoForPlugin,
  pluginMayUseDrive,
  pluginFetch,
  pluginForget,
  pluginChat,
  pluginLiveSend,
  pluginLocation,
  pluginMade,
  pluginOpen,
  pluginOpenChat,
  pluginPrint,
  pluginRead,
  pluginRecordForget,
  pluginRecordGet,
  pluginRecordKeys,
  pluginRecordSet,
  pluginRecordUsage,
  pluginSave,
  pluginWrite,
  remindCancel,
  remindList,
  remindSet,
  sendPicked,
  vaultBackup,
  vaultBackupInfo,
  vaultCancelPending,
  vaultConnect,
  vaultDisconnect,
  vaultDownload,
  vaultList,
  vaultMkdir,
  vaultMove,
  vaultOpen,
  vaultRemove,
  vaultRename,
  vaultRestore,
  vaultRetry,
  vaultSave,
  vaultStatus,
  vaultUploadMessage,
  vaultUploadPicked,
  PLUGIN_EVENT,
  type PickedFile,
  type PluginEvent,
  type Sending,
} from "../core";
import { i18n } from "../i18n";
import { CLOSING_WAIT, frameUrl, fromFrame, pluginName, type FrameMessage, type HandedFile } from "../plugins";
import { pluginTheme } from "../theme";

// Plan §53, §58: the plugin lives in its own frame, served from its own scheme with the policy its
// permissions allow. It never sees the app's window, the chat or the keys. It can be handed a text,
// it can ask the app for a file the user picks, and it can hand back a file or a text. How far
// that goes is what the user granted it (A2 of the 2026-09-24 review): with `propose` what it
// hands back lands in the composer and the user sends it; only with `auto` does it go out by
// itself; with nothing, nothing.
// 2026-09-27: it can also be opened with a file (`file`), with a way back to the message it came
// from (`reference`), from a reminder it set (`reminder`), and, with the `live` permission, it
// hears what its twin on the other side says and answers it. Without a contact (a plugin opened
// from Settings) there is no chat to write in and no other side.
// 2026-10-01 (§108): `session` is the hidden session it is open in (a conversation of that
// session, or a reminder set there). All it keeps, sets or looks up through the core belongs to
// that place; the core keeps each place apart and the plugin never hears of sessions.
// 2026-10-02: opened in a conversation, it learns the core's opaque id of it (`chat`), its own
// for this plugin, never who it is with.
const props = withDefaults(
  defineProps<{
    plugin: { id: string; name: string };
    contact: string;
    text?: string;
    sending?: Sending;
    file?: HandedFile;
    reference?: string;
    reminder?: string;
    live?: boolean;
    session?: string;
  }>(),
  {
    text: undefined,
    sending: "nothing",
    file: undefined,
    reference: undefined,
    reminder: undefined,
    live: false,
    session: undefined,
  },
);
// `done`: the plugin is finished or asked to be closed; whoever shows it decides, and closes it with
// `close()`. `closed`: it has said goodbye and can go.
const emit = defineEmits<{ text: [text: string]; attach: [file: PickedFile]; done: []; openChat: [contact: string]; closed: [] }>();

const frame = ref<HTMLIFrameElement | null>(null);
const height = ref(320);
// Requests still unanswered after WORKING_DELAY; the indicator shows while there is one.
const late = ref(0);
const working = computed(() => late.value > 0);

function tell(message: Record<string, unknown>) {
  frame.value?.contentWindow?.postMessage(message, "*");
}

async function onMessage(event: MessageEvent) {
  const said = fromFrame(event, frame.value);
  if (!said) return;
  if (closing && SILENT_WHILE_CLOSING.has(said.type)) return;

  if (said.type === "ft.hello") {
    tell({ type: "ft.theme", ...pluginTheme() });
  } else if (said.type === "ft.ready") {
    ready = true;
    tell({ ...opening(), ...(await chatOf()) });
  } else if (said.type === "ft.height") {
    height.value = Math.min(Math.max(said.height, 160), 4000);
  } else if (said.type === "ft.pickFile") {
    // The plugin never opens the picker: it asks, and the app asks the user (§53).
    await busy(async () => {
      try {
        // The core deletes the picker's copies, handed over or not (2026-10-02).
        const file = await pickForPlugin(said.accept ?? "");
        tell({ type: "ft.file", id: said.id, name: file?.name ?? "", mime: file?.mime ?? "", data: file?.data ?? "" });
      } catch {
        tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
      }
    });
  } else if (said.type === "ft.takePhoto") {
    // Like a pick, no manifest permission: the user takes the photo or backs out, so the user
    // decides what the plugin gets (2026-10-06). No camera, or none allowed, gives nothing.
    await busy(async () => {
      try {
        const photo = await takePhotoForPlugin();
        tell({ type: "ft.file", id: said.id, name: photo?.name ?? "", mime: photo?.mime ?? "", data: photo?.data ?? "" });
      } catch {
        tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
      }
    });
  } else if (said.type === "ft.made") {
    // Nothing leaves without the permission: the core checks it again on its side (A2).
    if (props.sending === "nothing") return void mayNotWrite();
    await busy(async () => {
      try {
        const made = await pluginMade(props.plugin.id, props.contact, said.name, said.mime, said.data);
        if (made.staged) emit("attach", made.staged);
        emit("done");
      } catch {
        // Refused by the core: the plugin's window stays, nothing was sent.
      }
    });
  } else if (said.type === "ft.text") {
    if (props.sending !== "nothing") emit("text", said.text);
    else void mayNotWrite();
  } else if (said.type === "ft.close") {
    // Asked again by a goodbye that ends with `ft.close()`: it is already closing.
    if (!closing) emit("done");
  } else if (said.type === "ft.closed") {
    letGo?.();
  } else {
    await answer(said);
  }
}

/**
 * app#76: a plugin that writes in the chat without the `send` permission is refused, and its main
 * action used to do nothing at all. The user is told why, and where to allow it. Opened outside a
 * conversation there is no chat to write in, so nothing to tell.
 */
async function mayNotWrite() {
  if (!props.contact) return;
  const toast = await toastController.create({ message: i18n.global.t("plugins.mayNotWrite"), duration: 4000, position: "bottom" });
  await toast.present();
}

/**
 * Closing (2026-10-02): a plugin closed by the app (its ✕, Android's Back, leaving the chat) used
 * to vanish without a word, and its twin on the other side kept saying both were there. Now it is
 * told (`ft.closing`, which runs its `ft.onClose`), and the frame is kept, out of sight, until it
 * answers `ft.closed` or for CLOSING_WAIT at most; only then does `closed` let it go. The window
 * is hidden at once by whoever shows it: nothing in the app waits for the plugin. Meanwhile it can
 * still talk to its twin and keep what it has, with the grants it had, and nothing more: what
 * would reach the user or the chat is ignored.
 */
let ready = false;
let closing: Promise<void> | undefined;
let letGo: (() => void) | undefined;
let gone = false;
const SILENT_WHILE_CLOSING = new Set<FrameMessage["type"]>([
  "ft.pickFile",
  "ft.takePhoto",
  "ft.made",
  "ft.text",
  "ft.openChat",
  "ft.drive",
  "ft.print",
  "ft.save",
  "ft.location",
]);

function close(): Promise<void> {
  closing ??= new Promise<void>((resolve) => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    letGo = () => {
      clearTimeout(timer);
      letGo = undefined;
      if (!gone) emit("closed");
      resolve();
    };
    // A plugin that never came up has heard nothing, started nothing and has nothing to say.
    if (!ready) return letGo();
    timer = setTimeout(letGo, CLOSING_WAIT);
    tell({ type: "ft.closing" });
  });
  return closing;
}
defineExpose({ close });

/** What the plugin is opened with: the text, the file, the way back, the language, the colours. */
function opening() {
  return {
    type: "ft.open",
    text: props.text ?? "",
    // Whether the app is dark and its colours (2026-10-02), as the frame puts them on its root.
    ...pluginTheme(),
    lang: i18n.global.locale.value,
    // A plain copy: the prop may be reactive state, and postMessage cannot clone a proxy.
    file: props.file ? { name: props.file.name, mime: props.file.mime, data: props.file.data } : null,
    ref: props.reference ?? null,
    reminder: props.reminder ?? null,
    live: Boolean(props.live && props.contact),
  };
}

/**
 * The chat it is opened in (2026-10-02), as the core's opaque id for this plugin, so what it keeps
 * per conversation stays with that conversation. Nothing without a contact, or when the core
 * gives no id (a blocked contact, a closed session): then `chat` is not there at all.
 */
async function chatOf(): Promise<{ chat?: string }> {
  if (!props.contact) return {};
  try {
    const chat = await pluginChat(props.plugin.id, props.contact);
    return typeof chat === "string" ? { chat } : {};
  } catch {
    return {};
  }
}

/** The questions the core answers for a plugin, each with the id it was asked with (§53). */
async function answer(said: Extract<FrameMessage, { id: string }>) {
  await busy(async () => {
    try {
      let value: unknown = true;
      const id = props.plugin.id;
      const session = props.session;
      if (said.type === "ft.save") await pluginSave(said.name, said.mime, said.data);
      else if (said.type === "ft.print") await pluginPrint(id, said.name, said.mime, said.data);
      else if (said.type === "ft.fetch") value = await pluginFetch(id, said.url, said.method, said.headers, said.body);
      else if (said.type === "ft.read") value = await pluginRead(id, said.key, session);
      else if (said.type === "ft.write") await pluginWrite(id, said.key, said.value, session);
      else if (said.type === "ft.forget") await pluginForget(id, said.key, session);
      else if (said.type === "ft.recordGet") value = await pluginRecordGet(id, said.key, session);
      else if (said.type === "ft.recordSet") await pluginRecordSet(id, said.key, said.value, session);
      else if (said.type === "ft.recordForget") await pluginRecordForget(id, said.key, session);
      else if (said.type === "ft.recordKeys") value = await pluginRecordKeys(id, said.prefix, session);
      else if (said.type === "ft.recordUsage") value = await pluginRecordUsage(id, session);
      else if (said.type === "ft.remindSet") await remindSet(id, said.reminder, said.at, said.text, session);
      else if (said.type === "ft.remindCancel") value = await remindCancel(id, said.reminder, session);
      else if (said.type === "ft.remindList") value = await remindList(id, session);
      else if (said.type === "ft.liveSend") value = props.live && props.contact ? await pluginLiveSend(id, props.contact, said.data) : false;
      else if (said.type === "ft.openChat") {
        const target = await pluginOpenChat(id, said.ref, session);
        if (target) emit("openChat", target.contact);
        value = Boolean(target);
      } else if (said.type === "ft.drive") value = await drive(said);
      else if (said.type === "ft.location") value = await pluginLocation(id);
      tell({ type: "ft.done", id: said.id, answer: value ?? null });
    } catch {
      // A position the plugin may not have is no position: `null`, as the contract says.
      tell({ type: "ft.done", id: said.id, answer: said.type === "ft.location" ? null : false });
    }
  });
}

/**
 * The user's cloud, for a plugin granted it (plan-drive): the core does everything; the plugin
 * sees names and sizes, never bytes, tokens or the code. `false` when it may not, or it failed.
 */
async function drive(said: Extract<FrameMessage, { type: "ft.drive" }>): Promise<unknown> {
  const id = props.plugin.id;
  if (!(await pluginMayUseDrive(id))) return false;
  const parent = (value: string) => value || null;
  switch (said.op) {
    case "status":
      return vaultStatus();
    case "connect":
      return vaultConnect(said.a || "google");
    // The recovery phrase is typed only in Settings → Backup (2026-09-28): a plugin can neither
    // set the drive up nor open it, so the phrase never crosses the frame.
    case "setup":
    case "unlock":
      return false;
    case "disconnect":
      await vaultDisconnect();
      return true;
    case "list":
      return vaultList(parent(said.a));
    case "mkdir":
      return vaultMkdir(said.a, parent(said.b));
    case "rename":
      await vaultRename(said.a, said.b);
      return true;
    case "move":
      await vaultMove(said.a, parent(said.b));
      return true;
    case "remove":
      await vaultRemove(said.a);
      return true;
    case "upload":
      // The plugin never opens the picker: the core asks the user, seals what they chose and
      // deletes the picker's copies (2026-10-02).
      return vaultUploadPicked(parent(said.a));
    case "keep": {
      // The file this plugin was opened with, by its ref: the bytes never pass through the frame.
      if (!props.reference) return false;
      const target = await pluginOpenChat(id, props.reference, props.session);
      if (!target) return false;
      await vaultUploadMessage(target.message, parent(said.a));
      return true;
    }
    case "open":
      await vaultOpen(said.a);
      return true;
    case "save":
      await vaultSave(said.a);
      return true;
    case "send": {
      // As any file a plugin makes (A2): sent with `auto`, staged for the user with `propose`.
      if (props.sending === "nothing" || !props.contact) return false;
      const file = await vaultDownload(said.a);
      if (props.sending === "auto") await sendPicked(props.contact, file);
      else emit("attach", file);
      emit("done");
      return true;
    }
    case "retry":
      return vaultRetry();
    case "cancel":
      await vaultCancelPending(said.a);
      return true;
    case "backup":
      return vaultBackup();
    case "backupInfo":
      return vaultBackupInfo();
    case "restore":
      return vaultRestore();
    default:
      return false;
  }
}

/** What the twin on the other side said, for this plugin and this conversation only. */
function onLive(event: PluginEvent) {
  if (props.live && event.plugin === props.plugin.id && event.contact === props.contact) {
    tell({ type: "ft.live", data: event.data });
  }
}
let unlisten: (() => void) | undefined;

/**
 * 2026-10-06: most requests (records, live moves) are answered in tens of milliseconds, and showing
 * the indicator for each one made the plugin flicker. Only a request still pending after
 * WORKING_DELAY (picking a file, a big save) shows it.
 */
const WORKING_DELAY = 400;

async function busy(work: () => Promise<void>) {
  let slow = false;
  const timer = setTimeout(() => {
    slow = true;
    late.value++;
  }, WORKING_DELAY);
  try {
    await work();
  } finally {
    clearTimeout(timer);
    if (slow) late.value--;
  }
}

/**
 * The app's look, followed while the plugin is open (2026-10-02): the root's class (`ft-dark`) and
 * `data-direction` are what the stylesheet reads, whoever changes them (Settings, or the system's
 * dark mode in `theme.ts`). The frame hears the colours again only when they really changed.
 */
let lastTheme = "";
const looks = new MutationObserver(() => {
  const now = pluginTheme();
  const said = JSON.stringify(now);
  if (said === lastTheme) return;
  lastTheme = said;
  tell({ type: "ft.theme", ...now });
});

// The core never updates a plugin under an open frame (2026-10-03): open from here until the
// sheet goes, its goodbye included.
const reportOpen = (id: string, open: boolean) => void pluginOpen(id, open).catch(() => undefined);
watch(
  () => props.plugin.id,
  (now, before) => {
    reportOpen(before, false);
    reportOpen(now, true);
  },
);

onMounted(async () => {
  reportOpen(props.plugin.id, true);
  lastTheme = JSON.stringify(pluginTheme());
  looks.observe(document.documentElement, { attributes: true, attributeFilter: ["class", "data-direction"] });
  window.addEventListener("message", onMessage);
  unlisten = await listen<PluginEvent>(PLUGIN_EVENT, ({ payload }) => onLive(payload)).catch(() => undefined);
});
onBeforeUnmount(() => {
  reportOpen(props.plugin.id, false);
  // Torn down without `close()` (a page that went): one word on the way out, without waiting. The
  // frame goes in this same tick, so it may well never hear it.
  gone = true;
  if (ready && !closing) tell({ type: "ft.closing" });
  letGo?.();
  looks.disconnect();
  window.removeEventListener("message", onMessage);
  // A bridge without listeners (tests, desktop) has nothing to unhook: that is not a failure.
  void Promise.resolve()
    .then(() => unlisten?.())
    .catch(() => undefined);
});
watch(
  () => [props.text, props.reminder],
  () => tell(opening()),
);
</script>

<template>
  <section class="ft-plugin">
    <!-- The name and the way out are the window's job; here only what the tool is doing. It floats
         over the frame's top edge: the plugin never moves when it shows or goes (2026-10-06). -->
    <span v-if="working" class="ft-plugin__working" role="status">…</span>
    <iframe
      ref="frame"
      class="ft-plugin__frame"
      :src="frameUrl(plugin.id)"
      :style="{ height: `${height}px` }"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      :title="pluginName(plugin)"
    />
  </section>
</template>

<style scoped>
.ft-plugin {
  position: relative;
  padding: 0 var(--ft-space-4) var(--ft-space-4);
}
.ft-plugin__working {
  position: absolute;
  z-index: 1;
  inset-block-start: var(--ft-space-2);
  inset-inline-end: var(--ft-space-4);
  padding: 0 var(--ft-space-3);
  border-radius: 999px;
  background: var(--ion-color-light);
  color: var(--ion-color-primary);
  pointer-events: none;
}
.ft-plugin__frame {
  display: block;
  width: 100%;
  border: 0;
  background: transparent;
}
</style>
