/**
 * Plugins in the app (issue app#3, Plan §53–§58). A plugin runs inside an iframe served from its
 * own scheme, with the policy its permissions allow, and only ever sees the text the user hands
 * it. It never touches the app's window, its storage or its keys.
 */
import { shallowRef } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { plugins, type PluginView } from "./core";

/**
 * Where the frame of a plugin lives; never the app's own origin. Only the id goes through
 * `convertFileSrc`, which would turn the slash of a path into `%2F`.
 */
export function frameUrl(id: string): string {
  return `${convertFileSrc(id, "ftplugin")}/frame.html`;
}

/**
 * The apps of this phone: every plugin installed, whatever it was granted (§53). One list for the
 * whole app, so a tool added or removed in Settings reaches a conversation that is already open.
 */
export const installed = shallowRef<PluginView[]>([]);

/** Asks the core what is installed and hands it to every screen that is watching. */
export async function refreshPlugins(): Promise<PluginView[]> {
  installed.value = await plugins().catch(() => []);
  return installed.value;
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
  | { type: "ft.forget"; id: string; key: string }
  // 2026-09-27: records, reminders, the live channel and the way back to a conversation.
  | { type: "ft.recordGet"; id: string; key: string }
  | { type: "ft.recordSet"; id: string; key: string; value: string }
  | { type: "ft.recordForget"; id: string; key: string }
  | { type: "ft.recordKeys"; id: string; prefix: string }
  | { type: "ft.recordUsage"; id: string }
  | { type: "ft.remindSet"; id: string; reminder: string; at: number; text: string }
  | { type: "ft.remindCancel"; id: string; reminder: string }
  | { type: "ft.remindList"; id: string }
  | { type: "ft.liveSend"; id: string; data: string }
  | { type: "ft.openChat"; id: string; ref: string }
  // The user's cloud (plan-drive): one question with an operation and up to two strings.
  | { type: "ft.drive"; id: string; op: DriveOp; a: string; b: string };

/** What a plugin may ask of the drive. Anything else is ignored. */
export const DRIVE_OPS = [
  "status", "connect", "setup", "unlock", "disconnect", "list", "mkdir", "rename", "move", "remove",
  "upload", "keep", "open", "save", "send", "retry", "cancel", "backup", "backupInfo", "restore",
] as const;
export type DriveOp = (typeof DRIVE_OPS)[number];

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
    case "ft.recordGet":
    case "ft.recordForget":
      return text(id) && text(said.key) ? { type: said.type, id, key: said.key } : null;
    case "ft.recordSet":
      return text(id) && text(said.key) && text(said.value) ? { type: "ft.recordSet", id, key: said.key, value: said.value } : null;
    case "ft.recordKeys":
      return text(id) ? { type: "ft.recordKeys", id, prefix: text(said.prefix) ? said.prefix : "" } : null;
    case "ft.recordUsage":
    case "ft.remindList":
      return text(id) ? { type: said.type, id } : null;
    case "ft.remindSet":
      return text(id) && text(said.reminder) && typeof said.at === "number" && Number.isFinite(said.at)
        ? { type: "ft.remindSet", id, reminder: said.reminder, at: said.at, text: text(said.text) ? said.text : "" }
        : null;
    case "ft.remindCancel":
      return text(id) && text(said.reminder) ? { type: "ft.remindCancel", id, reminder: said.reminder } : null;
    case "ft.liveSend":
      return text(id) && text(said.data) ? { type: "ft.liveSend", id, data: said.data } : null;
    case "ft.openChat":
      return text(id) && text(said.ref) ? { type: "ft.openChat", id, ref: said.ref } : null;
    case "ft.drive": {
      const op = DRIVE_OPS.find((one) => one === said.op);
      return text(id) && op ? { type: "ft.drive", id, op, a: text(said.a) ? said.a : "", b: text(said.b) ? said.b : "" } : null;
    }
    default:
      return null;
  }
}

/** A file as a plugin is handed it: name, kind and bytes as base64 (2026-09-27). */
export interface HandedFile {
  name: string;
  mime: string;
  data: string;
}

/** Whether a plugin says it opens a file of this kind (`opens` in its manifest). */
export function opensKind(opens: string[] | undefined, mime: string): boolean {
  const kind = mime.split(";")[0].trim().toLowerCase();
  const top = kind.split("/")[0];
  return (opens ?? []).some((one) => one === "*/*" || one === kind || one === `${top}/*`);
}

/**
 * The plugins that can open a message (2026-09-27): a text goes to those that open `text/plain`
 * and were granted reading what they are handed; a file, to those that open its kind.
 */
export function openersOf(plugins: PluginView[], message: { kind?: string; text?: string; file?: { mime?: string } }): PluginView[] {
  if (message.kind === "file") {
    const mime = message.file?.mime || "application/octet-stream";
    return plugins.filter((one) => opensKind(one.opens, mime));
  }
  return plugins.filter((one) => one.granted.messages && opensKind(one.opens, "text/plain"));
}

/**
 * The viewer of a file of this kind (2026-09-27): the plugin whose manifest `views` the exact
 * type. Opening (`opens`) is not viewing: the drive opens anything and the board opens pictures,
 * and neither is what a tap should show. Between two viewers, the one installed last.
 */
export function viewerOf(plugins: PluginView[], mime: string): PluginView | undefined {
  const kind = mime.split(";")[0].trim().toLowerCase();
  return plugins
    .filter((one) => (one.views ?? []).includes(kind))
    .sort((a, b) => b.installedAt - a.installedAt)[0];
}
