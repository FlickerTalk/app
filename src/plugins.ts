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

/** The apps of this phone: every plugin installed, whatever it was granted (§53). */
export async function installedPlugins(): Promise<PluginView[]> {
  return await plugins();
}

/**
 * What a plugin may say to the app (§53). Four of them are things it hands over —it is ready, how
 * tall it is, a file it made, a text it proposes— and the rest are questions the core answers:
 * a file the user picks, saving, printing, a call to a host it was granted, and its own memory.
 * Anything else is ignored.
 */
export type FrameMessage =
  | { type: "ft.ready" }
  | { type: "ft.close" }
  | { type: "ft.height"; height: number }
  | { type: "ft.pickFile"; id: string; accept?: string }
  | { type: "ft.made"; name: string; mime: string; data: string }
  | { type: "ft.text"; text: string }
  | { type: "ft.save"; id: string; name: string; mime: string; data: string }
  | { type: "ft.print"; id: string; name: string; mime: string; data: string }
  | { type: "ft.fetch"; id: string; url: string; method: string; headers: [string, string][]; body: string | null }
  | { type: "ft.read"; id: string; key: string }
  | { type: "ft.write"; id: string; key: string; value: string }
  | { type: "ft.forget"; id: string; key: string };

const text = (value: unknown): value is string => typeof value === "string";

/** Only the frame of this plugin is listened to, and only for what it may say. */
export function fromFrame(event: MessageEvent, frame: HTMLIFrameElement | null): FrameMessage | null {
  if (!frame || event.source !== frame.contentWindow) return null;
  const said = event.data as Record<string, unknown> | null;
  if (!said || !text(said.type)) return null;
  const id = said.id;
  const file = { name: said.name, mime: said.mime, data: said.data };
  const isFile = text(file.name) && text(file.mime) && text(file.data);

  switch (said.type) {
    case "ft.ready":
      return { type: "ft.ready" };
    case "ft.close":
      return { type: "ft.close" };
    case "ft.height":
      return typeof said.height === "number" ? { type: "ft.height", height: said.height } : null;
    case "ft.pickFile":
      return text(id) ? { type: "ft.pickFile", id, ...(text(said.accept) ? { accept: said.accept } : {}) } : null;
    case "ft.made":
      return isFile ? { type: "ft.made", name: file.name as string, mime: file.mime as string, data: file.data as string } : null;
    case "ft.text":
      return text(said.text) ? { type: "ft.text", text: said.text } : null;
    case "ft.save":
    case "ft.print":
      return text(id) && isFile
        ? {
            type: said.type,
            id,
            name: file.name as string,
            mime: file.mime as string,
            data: file.data as string,
          }
        : null;
    case "ft.fetch": {
      const headers = Array.isArray(said.headers) ? (said.headers as [string, string][]) : null;
      if (!text(id) || !text(said.url) || !text(said.method) || !headers) return null;
      return { type: "ft.fetch", id, url: said.url, method: said.method, headers, body: text(said.body) ? said.body : null };
    }
    case "ft.read":
      return text(id) && text(said.key) ? { type: "ft.read", id, key: said.key } : null;
    case "ft.write":
      return text(id) && text(said.key) && text(said.value)
        ? { type: "ft.write", id, key: said.key, value: said.value }
        : null;
    case "ft.forget":
      return text(id) && text(said.key) ? { type: "ft.forget", id, key: said.key } : null;
    default:
      return null;
  }
}
