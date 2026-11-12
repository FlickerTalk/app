import { computed, type Ref } from "vue";
import { openFile, pluginRef, readMessageFile, type ChatMessage, type PluginView } from "./core";
import { byPluginName, installed, isLocked, openersOf, pluginName, viewerOf } from "./plugins";
import type { Opening } from "./pluginWindow";

export function useOpenWith(options: {
  messages: () => ChatMessage[];
  acting: Ref<string>;
  closeActions: () => void;
  openPlugin: (chosen: Opening) => void;
  allowedToRead: (chosen: PluginView) => Promise<PluginView | undefined>;
  toPremium: () => void;
}) {
  const openers = computed(() => {
    const message = options.messages().find((one) => one.id === options.acting.value);
    return message ? byPluginName(openersOf(installed.value, message)) : [];
  });

  async function openWith(id: string) {
    const message = options.messages().find((one) => one.id === options.acting.value);
    let chosen = installed.value.find((one) => one.id === id);
    options.closeActions();
    if (!message || !chosen) return;
    if (isLocked(chosen)) return options.toPremium();
    if (message.kind !== "file" && !chosen.granted.messages) chosen = await options.allowedToRead(chosen);
    if (chosen) await openIn(chosen, message);
  }

  function viewerFor(message: ChatMessage | undefined): PluginView | undefined {
    const viewer = message?.kind === "file" ? viewerOf(installed.value, message.file?.mime || "application/octet-stream") : undefined;
    return viewer && !isLocked(viewer) ? viewer : undefined;
  }
  const viewable = computed(() => Boolean(viewerFor(options.messages().find((one) => one.id === options.acting.value))));

  async function tapFile(id: string) {
    const message = options.messages().find((one) => one.id === id);
    const viewer = viewerFor(message);
    if (message && viewer && (await openIn(viewer, message))) return;
    await openFile(id);
  }

  async function openElsewhere() {
    const id = options.acting.value;
    options.closeActions();
    await openFile(id);
  }

  async function openIn(chosen: PluginView, message: ChatMessage): Promise<boolean> {
    try {
      // a drive-granted plugin gets the ref, the bytes never cross the frame
      const file =
        message.kind !== "file"
          ? undefined
          : chosen.granted.drive
            ? { name: message.file?.name ?? "", mime: message.file?.mime ?? "application/octet-stream", data: "" }
            : await readMessageFile(message.id);
      const reference = await pluginRef(chosen.id, message.id).catch(() => undefined);
      options.openPlugin({
        id: chosen.id,
        name: pluginName(chosen),
        sending: chosen.granted.send,
        live: Boolean(chosen.granted.live),
        text: message.kind === "file" ? undefined : message.text,
        file,
        reference,
      });
      return true;
    } catch {
      return false;
    }
  }

  return { openers, viewable, openWith, openElsewhere, tapFile };
}
