/**
 * Plugins in the app (issue app#3, Plan §53–§58). A plugin runs inside an iframe served from its
 * own scheme, with the policy its permissions allow, and only ever sees the text the user hands
 * it. It never touches the app's window, its storage or its keys.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { plugins, type PluginView } from "./core";

/**
 * Where the frame of a plugin lives; never the app's own origin. Only the id goes through
 * `convertFileSrc`, which would turn the slash of a path into `%2F`.
 */
export function frameUrl(id: string): string {
  return `${convertFileSrc(id, "ftplugin")}/frame.html`;
}

/** The plugins the user allowed to read what they are handed. */
export async function readyPlugins(): Promise<PluginView[]> {
  return (await plugins()).filter((plugin) => plugin.granted.messages);
}

/**
 * What a plugin may say to the app (§53): it is ready, how tall it is, "give me a file the user
 * picks", "here is the file I made" or "put this text in the chat". Anything else is ignored.
 */
export type FrameMessage =
  | { type: "ft.ready" }
  | { type: "ft.height"; height: number }
  | { type: "ft.pickFile"; accept?: string }
  | { type: "ft.made"; name: string; mime: string; data: string }
  | { type: "ft.text"; text: string };

/** Only the frame of this plugin is listened to, and only for what it may say. */
export function fromFrame(event: MessageEvent, frame: HTMLIFrameElement | null): FrameMessage | null {
  if (!frame || event.source !== frame.contentWindow) return null;
  const said = event.data as Partial<FrameMessage> & { type?: string };
  if (!said || typeof said.type !== "string") return null;

  switch (said.type) {
    case "ft.ready":
      return { type: "ft.ready" };
    case "ft.height":
      return typeof (said as { height?: unknown }).height === "number"
        ? { type: "ft.height", height: (said as { height: number }).height }
        : null;
    case "ft.pickFile": {
      const accept = (said as { accept?: unknown }).accept;
      return { type: "ft.pickFile", ...(typeof accept === "string" ? { accept } : {}) };
    }
    case "ft.made": {
      const made = said as { name?: unknown; mime?: unknown; data?: unknown };
      if (typeof made.name !== "string" || typeof made.mime !== "string" || typeof made.data !== "string") return null;
      return { type: "ft.made", name: made.name, mime: made.mime, data: made.data };
    }
    case "ft.text": {
      const text = (said as { text?: unknown }).text;
      return typeof text === "string" ? { type: "ft.text", text } : null;
    }
    default:
      return null;
  }
}
