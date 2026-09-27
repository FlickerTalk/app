<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { listen } from "@tauri-apps/api/event";
import {
  pickFiles,
  pluginFetch,
  pluginForget,
  pluginLiveSend,
  pluginMade,
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
  readPicked,
  remindCancel,
  remindList,
  remindSet,
  PLUGIN_EVENT,
  type PickedFile,
  type PluginEvent,
  type Sending,
} from "../core";
import { i18n } from "../i18n";
import { frameUrl, fromFrame, type FrameMessage, type HandedFile } from "../plugins";

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
  }>(),
  { text: undefined, sending: "nothing", file: undefined, reference: undefined, reminder: undefined, live: false },
);
const emit = defineEmits<{ text: [text: string]; attach: [file: PickedFile]; done: []; openChat: [contact: string] }>();

const frame = ref<HTMLIFrameElement | null>(null);
const height = ref(320);
const working = ref(false);

function tell(message: Record<string, unknown>) {
  frame.value?.contentWindow?.postMessage(message, "*");
}

async function onMessage(event: MessageEvent) {
  const said = fromFrame(event, frame.value);
  if (!said) return;

  if (said.type === "ft.ready") {
    tell(opening());
  } else if (said.type === "ft.height") {
    height.value = Math.min(Math.max(said.height, 160), 4000);
  } else if (said.type === "ft.pickFile") {
    // The plugin never opens the picker: it asks, and the app asks the user (§53).
    await busy(async () => {
      try {
        const [file] = await pickFiles(said.accept ?? "");
        const data = file ? await readPicked(file.path) : "";
        tell({ type: "ft.file", id: said.id, name: file?.name ?? "", mime: file?.mime ?? "", data });
      } catch {
        tell({ type: "ft.file", id: said.id, name: "", mime: "", data: "" });
      }
    });
  } else if (said.type === "ft.made") {
    // Nothing leaves without the permission: the core checks it again on its side (A2).
    if (props.sending === "nothing") return;
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
  } else if (said.type === "ft.close") {
    emit("done");
  } else {
    await answer(said);
  }
}

/** What the plugin is opened with: the text, the file, the way back, the language, the colours. */
function opening() {
  return {
    type: "ft.open",
    text: props.text ?? "",
    dark: dark(),
    lang: i18n.global.locale.value,
    file: props.file ?? null,
    ref: props.reference ?? null,
    reminder: props.reminder ?? null,
    live: Boolean(props.live && props.contact),
  };
}

/** The questions the core answers for a plugin, each with the id it was asked with (§53). */
async function answer(said: Extract<FrameMessage, { id: string }>) {
  await busy(async () => {
    try {
      let value: unknown = true;
      const id = props.plugin.id;
      if (said.type === "ft.save") await pluginSave(said.name, said.mime, said.data);
      else if (said.type === "ft.print") await pluginPrint(id, said.name, said.mime, said.data);
      else if (said.type === "ft.fetch") value = await pluginFetch(id, said.url, said.method, said.headers, said.body);
      else if (said.type === "ft.read") value = await pluginRead(id, said.key);
      else if (said.type === "ft.write") await pluginWrite(id, said.key, said.value);
      else if (said.type === "ft.forget") await pluginForget(id, said.key);
      else if (said.type === "ft.recordGet") value = await pluginRecordGet(id, said.key);
      else if (said.type === "ft.recordSet") await pluginRecordSet(id, said.key, said.value);
      else if (said.type === "ft.recordForget") await pluginRecordForget(id, said.key);
      else if (said.type === "ft.recordKeys") value = await pluginRecordKeys(id, said.prefix);
      else if (said.type === "ft.recordUsage") value = await pluginRecordUsage(id);
      else if (said.type === "ft.remindSet") await remindSet(id, said.reminder, said.at, said.text);
      else if (said.type === "ft.remindCancel") value = await remindCancel(id, said.reminder);
      else if (said.type === "ft.remindList") value = await remindList(id);
      else if (said.type === "ft.liveSend") value = props.live && props.contact ? await pluginLiveSend(id, props.contact, said.data) : false;
      else if (said.type === "ft.openChat") {
        const target = await pluginOpenChat(id, said.ref);
        if (target) emit("openChat", target.contact);
        value = Boolean(target);
      }
      tell({ type: "ft.done", id: said.id, answer: value ?? null });
    } catch {
      tell({ type: "ft.done", id: said.id, answer: false });
    }
  });
}

/** What the twin on the other side said, for this plugin and this conversation only. */
function onLive(event: PluginEvent) {
  if (props.live && event.plugin === props.plugin.id && event.contact === props.contact) {
    tell({ type: "ft.live", data: event.data });
  }
}
let unlisten: (() => void) | undefined;

async function busy(work: () => Promise<void>) {
  working.value = true;
  try {
    await work();
  } finally {
    working.value = false;
  }
}

/** Whether the app is showing dark, so the plugin can paint like the rest of the app. */
function dark(): boolean {
  return document.documentElement.classList.contains("ion-palette-dark");
}

onMounted(async () => {
  window.addEventListener("message", onMessage);
  unlisten = await listen<PluginEvent>(PLUGIN_EVENT, ({ payload }) => onLive(payload)).catch(() => undefined);
});
onBeforeUnmount(() => {
  window.removeEventListener("message", onMessage);
  // A bridge without listeners (tests, desktop) has nothing to unhook: that is not a failure.
  void Promise.resolve()
    .then(() => unlisten?.())
    .catch(() => undefined);
});
watch(
  () => props.text,
  () => tell(opening()),
);
</script>

<template>
  <section class="ft-plugin">
    <!-- The name and the way out are the window's job; here only what the tool is doing. -->
    <p v-if="working" class="ft-plugin__working" role="status">…</p>
    <iframe
      ref="frame"
      class="ft-plugin__frame"
      :src="frameUrl(plugin.id)"
      :style="{ height: `${height}px` }"
      sandbox="allow-scripts"
      referrerpolicy="no-referrer"
      :title="plugin.name"
    />
  </section>
</template>

<style scoped>
.ft-plugin {
  padding: 0 var(--ft-space-4) var(--ft-space-4);
}
.ft-plugin__working {
  margin: 0 0 var(--ft-space-2);
  color: var(--ft-accent);
}
.ft-plugin__frame {
  display: block;
  width: 100%;
  border: 0;
  background: transparent;
}
</style>
