/**
 * Plugins in the app (issue app#3, Plan §53–§58). A plugin runs inside an iframe served from its
 * own scheme, with the policy its permissions allow, and only ever sees the text the user hands
 * it. It never touches the app's window, its storage or its keys.
 */
import { computed, shallowRef } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { offeredPlugins, plugins, type OfferedPlugin, type PluginLocales, type PluginView } from "./core";
import { isGame } from "./games";
import { i18n } from "./i18n";

/**
 * Where the frame of a plugin lives; never the app's own origin. Only the id goes through
 * `convertFileSrc`, which would turn the slash of a path into `%2F`.
 */
export function frameUrl(id: string): string {
  // The version installed (2026-10-03): after an update, the next opening never gets the old
  // code from a cache by the same address.
  const version = installed.value.find((one) => one.id === id)?.version;
  return `${convertFileSrc(id, "ftplugin")}/frame.html${version ? `?v=${encodeURIComponent(version)}` : ""}`;
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

/** What the core says when the plugins installed here changed: an update made in the background. */
export const PLUGINS_EVENT = "ft://plugins";

/**
 * Reads the plugins again whenever the core says they changed (2026-10-03): an update it made in
 * the background reaches every screen. Returns how to stop listening.
 */
export async function followPluginChanges(): Promise<() => void> {
  return listen(PLUGINS_EVENT, () => void refreshPlugins());
}

/**
 * The tools of the chat and the games, apart (plan 10.3): the tools sheet, "open with" and
 * Settings show tools only; games have their own section. Anything not marked a game is a tool.
 */
export const tools = computed(() => byPluginName(installed.value.filter((one) => !isGame(one))));
export const games = computed(() => byPluginName(installed.value.filter(isGame)));

/**
 * What the catalogue offers this phone, kept for every screen (plan 10.6): the games section
 * reads it, and so does a bubble with an invitation, which must never fetch it by itself.
 */
export const offered = shallowRef<OfferedPlugin[]>([]);
export const offeredGames = computed(() => byPluginName(offered.value.filter(isGame)));

/** Asks the core what the catalogue offers. On failure what was known stays, and the error goes up. */
export async function refreshOffered(): Promise<OfferedPlugin[]> {
  offered.value = await offeredPlugins();
  return offered.value;
}

/** What the screens need of a plugin to name it: an installed one, an offered one, or just its id. */
interface Named {
  id: string;
  name: string;
  summary?: string;
  locales?: PluginLocales;
}

/**
 * A plugin's name or summary in the phone's language (2026-10-02, plan of the catalogue's
 * translations): the exact language, then its base language (`zh` for `zh-TW`); for each, the
 * installed package's own, then the catalogue's entry with the same id. Otherwise the English one.
 * It comes from a package: shown as text, never as HTML.
 */
function localized(plugin: Named, field: "name" | "summary"): string {
  const locale = i18n.global.locale.value;
  const sources = [
    installed.value.find((one) => one.id === plugin.id),
    offered.value.find((one) => one.id === plugin.id),
    plugin,
  ] as (Named | undefined)[];
  const languages = locale === "en" ? [] : [...new Set([locale, locale.split("-")[0]])];
  for (const language of languages) {
    for (const source of sources) {
      const said = source?.locales?.[language]?.[field];
      if (said?.trim()) return said;
    }
  }
  // The English one: of this view, or of the other one when this view has none (an installed
  // plugin's view carries no summary).
  return plugin[field] || sources.find((source) => source?.[field])?.[field] || "";
}

export function pluginName(plugin: Named): string {
  return localized(plugin, "name");
}

export function pluginSummary(plugin: Named): string {
  return localized(plugin, "summary");
}

/** A copy sorted by the name the phone shows, in the phone's own order (the core sorts by the English). */
export function byPluginName<T extends Named>(list: readonly T[]): T[] {
  const order = new Intl.Collator(i18n.global.locale.value);
  return [...list].sort((a, b) => order.compare(pluginName(a), pluginName(b)));
}

let asking: Promise<OfferedPlugin[]> | null = null;
/** The list `offeredOnce` last read; a screen that refreshes the list itself replaces it. */
let lastAsked: OfferedPlugin[] | null = null;

/**
 * What the catalogue offers, for an invitation to a game this phone does not have: asked once per
 * run, and once at a time, however many games are known. The seeds include the games
 * (2026-10-03), so games are always known; a game the app does not carry is only in the
 * catalogue. A screen that refreshes the list itself (the games tab) lets the next look ask again.
 * Offline, the core answers with what the app carries.
 */
export async function offeredOnce(): Promise<OfferedPlugin[]> {
  if (lastAsked && offered.value === lastAsked) return offered.value;
  asking ??= refreshOffered()
    .then((list) => (lastAsked = list))
    .catch(() => offered.value)
    .finally(() => (asking = null));
  return asking;
}

/**
 * What a plugin may say to the app (§53). Four of them are things it hands over —it is ready, how
 * tall it is, a file it made, a text it proposes— and the rest are questions the core answers:
 * a file the user picks, saving, printing, a call to a host it was granted, and its own memory.
 * Anything else is ignored.
 */
export type FrameMessage =
  | { type: "ft.ready" }
  /** The frame asks for the app's colours before it loads the plugin (2026-10-02). */
  | { type: "ft.hello" }
  | { type: "ft.close" }
  /** The answer to `ft.closing` (2026-10-02): the plugin has said goodbye. */
  | { type: "ft.closed" }
  | { type: "ft.height"; height: number }
  | { type: "ft.pickFile"; id: string; accept?: string }
  /** 2026-10-06: a photo taken now with the phone's camera app, answered like `ft.pickFile`. */
  | { type: "ft.takePhoto"; id: string }
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
  // 2026-10-02: where the phone is, once.
  | { type: "ft.location"; id: string }
  // The user's cloud (plan-drive): one question with an operation and up to two strings.
  | { type: "ft.drive"; id: string; op: DriveOp; a: string; b: string };

/**
 * How long a closing plugin is kept, out of sight, to say goodbye to its twin (2026-10-02): its
 * window goes at once, and the frame goes when the plugin answers `ft.closed` or after this, in ms.
 */
export const CLOSING_WAIT = 400;

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
    case "ft.hello":
      return { type: "ft.hello" };
    case "ft.close":
      return { type: "ft.close" };
    case "ft.closed":
      return { type: "ft.closed" };
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
    case "ft.location":
    case "ft.takePhoto":
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
  const candidates = plugins.filter((one) => !isGame(one));
  if (message.kind === "file") {
    const mime = message.file?.mime || "application/octet-stream";
    return candidates.filter((one) => opensKind(one.opens, mime));
  }
  return candidates.filter((one) => one.granted.messages && opensKind(one.opens, "text/plain"));
}

/**
 * The viewer of a file of this kind (2026-09-27): the plugin whose manifest `views` the exact
 * type. Opening (`opens`) is not viewing: the drive opens anything and the board opens pictures,
 * and neither is what a tap should show. Between two viewers, the one installed last.
 */
export function viewerOf(plugins: PluginView[], mime: string): PluginView | undefined {
  const kind = mime.split(";")[0].trim().toLowerCase();
  return plugins
    .filter((one) => !isGame(one) && (one.views ?? []).includes(kind))
    .sort((a, b) => b.installedAt - a.installedAt)[0];
}
