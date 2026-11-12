import { computed, shallowRef } from "vue";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  PLAN_EVENT,
  offeredPlugins,
  plan,
  plugins,
  type OfferedPlugin,
  type PluginHookName,
  type PluginLocales,
  type PluginSettingsField,
  type PluginSettingsLabels,
  type PluginView,
} from "./core";
import { isGame } from "./games";
import { i18n } from "./i18n";
import {
  addOutline, alarmOutline, albumsOutline, appsOutline, arrowBackOutline, arrowRedoOutline, arrowUndoOutline,
  arrowUpOutline, basketOutline, boatOutline, bookOutline, brushOutline, calculatorOutline, calendarOutline,
  cameraOutline, chatbubbleOutline, checkboxOutline, checkmarkOutline, closeOutline, cloudDoneOutline, cloudOutline,
  cloudUploadOutline, codeSlashOutline, colorPaletteOutline, constructOutline, contrastOutline, cropOutline,
  cubeOutline, cutOutline, diceOutline, documentOutline, documentTextOutline, downloadOutline, easelOutline,
  ellipseOutline, ellipsisHorizontalOutline, expandOutline, extensionPuzzleOutline, eyeOffOutline, eyeOutline,
  folderOpenOutline, folderOutline, gameControllerOutline, gridOutline, handLeftOutline, imageOutline, keyOutline,
  languageOutline, linkOutline, listOutline, locationOutline, lockClosedOutline, logoMarkdown, mapOutline,
  micOutline, moveOutline, musicalNotesOutline, optionsOutline, pauseOutline, pencilOutline, playOutline,
  qrCodeOutline, radioButtonOnOutline, readerOutline, refreshOutline, removeOutline, resizeOutline, saveOutline,
  scanOutline, searchOutline, sendOutline, shieldOutline, sparklesOutline, squareOutline, statsChartOutline,
  textOutline, timeOutline, timerOutline, trashOutline, trophyOutline,
} from "ionicons/icons";

// only the id goes through convertFileSrc, it would turn a path's slash into %2F
export function frameUrl(id: string): string {
  // the version in the URL, so an update never loads cached old code
  const version = installed.value.find((one) => one.id === id)?.version;
  return `${convertFileSrc(id, "ftplugin")}/frame.html${version ? `?v=${encodeURIComponent(version)}` : ""}`;
}

export const installed = shallowRef<PluginView[]>([]);

let firstRead: Promise<PluginView[]> | undefined;

export function refreshPlugins(): Promise<PluginView[]> {
  const read = plugins()
    .catch(() => [])
    .then((list) => (installed.value = list));
  firstRead ??= read;
  return read;
}

export function pluginsReady(): Promise<PluginView[]> {
  return firstRead ?? refreshPlugins();
}

export const PLUGINS_EVENT = "ft://plugins";

export async function followPluginChanges(): Promise<() => void> {
  void refreshPlugins();
  return listen(PLUGINS_EVENT, () => void refreshPlugins());
}

// the core enforces the lock, this only drives the UI
export const premiumLocked = shallowRef(false);

export async function refreshPremiumLock(): Promise<boolean> {
  premiumLocked.value = (await plan().catch(() => null))?.state === "limited";
  return premiumLocked.value;
}

export async function followPremiumLock(): Promise<() => void> {
  void refreshPremiumLock();
  return listen(PLAN_EVENT, () => void refreshPremiumLock());
}

export function isLocked(plugin: { kind?: "tool" | "game" }): boolean {
  return premiumLocked.value && !isGame(plugin);
}

export const tools = computed(() => byPluginName(installed.value.filter((one) => !isGame(one))));
export const games = computed(() => byPluginName(installed.value.filter(isGame)));

// a whitelist, a name not here draws the app's own icon
export const PLUGIN_ICONS: Readonly<Record<string, string>> = Object.freeze(
  Object.assign(Object.create(null) as Record<string, string>, {
    "add-outline": addOutline, "alarm-outline": alarmOutline, "albums-outline": albumsOutline,
    "apps-outline": appsOutline, "arrow-back-outline": arrowBackOutline, "arrow-redo-outline": arrowRedoOutline,
    "arrow-undo-outline": arrowUndoOutline, "arrow-up-outline": arrowUpOutline, "basket-outline": basketOutline,
    "boat-outline": boatOutline, "book-outline": bookOutline, "brush-outline": brushOutline,
    "calculator-outline": calculatorOutline, "calendar-outline": calendarOutline, "camera-outline": cameraOutline,
    "chatbubble-outline": chatbubbleOutline, "checkbox-outline": checkboxOutline, "checkmark-outline": checkmarkOutline,
    "close-outline": closeOutline, "cloud-done-outline": cloudDoneOutline, "cloud-outline": cloudOutline,
    "cloud-upload-outline": cloudUploadOutline, "code-slash-outline": codeSlashOutline,
    "color-palette-outline": colorPaletteOutline, "construct-outline": constructOutline,
    "contrast-outline": contrastOutline, "crop-outline": cropOutline, "cube-outline": cubeOutline,
    "cut-outline": cutOutline, "dice-outline": diceOutline, "document-outline": documentOutline,
    "document-text-outline": documentTextOutline, "download-outline": downloadOutline, "easel-outline": easelOutline,
    "ellipse-outline": ellipseOutline, "ellipsis-horizontal-outline": ellipsisHorizontalOutline,
    "expand-outline": expandOutline, "extension-puzzle-outline": extensionPuzzleOutline,
    "eye-off-outline": eyeOffOutline, "eye-outline": eyeOutline, "folder-open-outline": folderOpenOutline,
    "folder-outline": folderOutline, "game-controller-outline": gameControllerOutline, "grid-outline": gridOutline,
    "hand-left-outline": handLeftOutline, "image-outline": imageOutline, "key-outline": keyOutline,
    "language-outline": languageOutline, "link-outline": linkOutline, "list-outline": listOutline,
    "location-outline": locationOutline, "lock-closed-outline": lockClosedOutline, "logo-markdown": logoMarkdown,
    "map-outline": mapOutline, "mic-outline": micOutline, "move-outline": moveOutline,
    "musical-notes-outline": musicalNotesOutline, "options-outline": optionsOutline, "pause-outline": pauseOutline,
    "pencil-outline": pencilOutline, "play-outline": playOutline, "qr-code-outline": qrCodeOutline,
    "radio-button-on-outline": radioButtonOnOutline, "reader-outline": readerOutline,
    "refresh-outline": refreshOutline, "remove-outline": removeOutline, "resize-outline": resizeOutline,
    "save-outline": saveOutline, "scan-outline": scanOutline, "search-outline": searchOutline,
    "send-outline": sendOutline, "shield-outline": shieldOutline, "sparkles-outline": sparklesOutline,
    "square-outline": squareOutline, "stats-chart-outline": statsChartOutline, "text-outline": textOutline,
    "time-outline": timeOutline, "timer-outline": timerOutline, "trash-outline": trashOutline,
    "trophy-outline": trophyOutline,
  }),
);

export function pluginIcon(plugin: { icon?: string; kind?: "tool" | "game" }): string {
  // the map has no prototype, so toString or __proto__ is no icon
  const named = plugin.icon && plugin.icon in PLUGIN_ICONS ? PLUGIN_ICONS[plugin.icon] : undefined;
  return named ?? (isGame(plugin) ? gameControllerOutline : extensionPuzzleOutline);
}

// as an <img>, so nothing in the SVG runs
export function pluginImage(plugin: { image?: string }): string | undefined {
  if (!plugin.image) return undefined;
  const bytes = new TextEncoder().encode(plugin.image);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:image/svg+xml;base64,${btoa(binary)}`;
}

// a bubble with an invitation must never fetch the catalogue itself
export const offered = shallowRef<OfferedPlugin[]>([]);
export const offeredGames = computed(() => byPluginName(offered.value.filter(isGame)));

export async function refreshOffered(): Promise<OfferedPlugin[]> {
  offered.value = await offeredPlugins();
  return offered.value;
}

interface Named {
  id: string;
  name: string;
  summary?: string;
  locales?: PluginLocales;
}

// from the package, shown as text, never as HTML
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
  return plugin[field] || sources.find((source) => source?.[field])?.[field] || "";
}

export function pluginName(plugin: Named): string {
  return localized(plugin, "name");
}

export function hooked(name: PluginHookName): PluginView[] {
  return tools.value.filter((one) => Boolean(one.hooks?.[name]));
}

function hookSaid(plugin: PluginView, name: PluginHookName, english: string, pick: (said: string | PluginSettingsLabels) => unknown): string {
  const locale = i18n.global.locale.value;
  const languages = locale === "en" ? [] : [...new Set([locale, locale.split("-")[0]])];
  for (const language of languages) {
    const labels = plugin.locales?.[language]?.hooks?.[name];
    const said = labels === undefined ? undefined : pick(labels);
    if (typeof said === "string" && said.trim()) return said;
  }
  return english;
}

export function hookLabel(plugin: PluginView, name: PluginHookName): string {
  const english = plugin.hooks?.[name]?.label ?? "";
  if (!english) return "";
  return hookSaid(plugin, name, english, (said) => (typeof said === "string" ? said : said.label));
}

export function settingsFields(plugin: PluginView): PluginSettingsField[] {
  return (plugin.hooks?.settings?.fields ?? []).map((field) => ({
    ...field,
    label: hookSaid(plugin, "settings", field.label, (said) => (typeof said === "string" ? undefined : said.fields?.[field.key])),
  }));
}

export function settingsNote(plugin: PluginView): string {
  const english = plugin.hooks?.settings?.note ?? "";
  if (!english) return "";
  return hookSaid(plugin, "settings", english, (said) => (typeof said === "string" ? undefined : said.note));
}

export function hookIcon(plugin: PluginView, name: PluginHookName): string {
  return pluginIcon({ icon: plugin.hooks?.[name]?.icon, kind: "tool" });
}

export function pluginSummary(plugin: Named): string {
  return localized(plugin, "summary");
}

export function byPluginName<T extends Named>(list: readonly T[]): T[] {
  const order = new Intl.Collator(i18n.global.locale.value);
  return [...list].sort((a, b) => order.compare(pluginName(a), pluginName(b)));
}

let asking: Promise<OfferedPlugin[]> | null = null;
let lastAsked: OfferedPlugin[] | null = null;

export async function offeredOnce(): Promise<OfferedPlugin[]> {
  if (lastAsked && offered.value === lastAsked) return offered.value;
  asking ??= refreshOffered()
    .then((list) => (lastAsked = list))
    .catch(() => offered.value)
    .finally(() => (asking = null));
  return asking;
}

export type FrameMessage =
  | { type: "ft.ready" }
  | { type: "ft.hello" }
  | { type: "ft.close" }
  | { type: "ft.closed" }
  | { type: "ft.height"; height: number }
  | { type: "ft.pickFile"; id: string; accept?: string }
  | { type: "ft.takePhoto"; id: string }
  | { type: "ft.made"; name: string; mime: string; data: string }
  | { type: "ft.text"; text: string }
  | { type: "ft.notify"; text: string; sticky: boolean }
  | { type: "ft.save"; id: string; name: string; mime: string; data: string }
  | { type: "ft.print"; id: string; name: string; mime: string; data: string }
  | { type: "ft.fetch"; id: string; url: string; method: string; headers: [string, string][]; body: string | null }
  | { type: "ft.read"; id: string; key: string }
  | { type: "ft.write"; id: string; key: string; value: string }
  | { type: "ft.forget"; id: string; key: string }
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
  | { type: "ft.location"; id: string }
  | { type: "ft.translateLanguages"; id: string }
  | { type: "ft.translateDetect"; id: string; text: string }
  | { type: "ft.translateStatus" | "ft.translatePrepare" | "ft.translateRemove"; id: string; from: string; to: string }
  | { type: "ft.translateText"; id: string; text: string; from: string; to: string }
  | { type: "ft.drive"; id: string; op: DriveOp; a: string; b: string }
  | { type: "ft.hookDone"; id: string; result: PluginHookResult | null }
  | { type: "ft.show" };

// `chat` is pluginChat's key
export interface PluginHook {
  name: PluginHookName;
  text: string;
  chat: string;
  ref: string | null;
  lang: string;
  manual: boolean;
  answer?: boolean;
  values?: Record<string, string | boolean>;
}

export type PluginPlatform = "android" | "ios" | "desktop";

// iPhones show their own download sheet and Android doesn't, so tools only ask there
export function hookPlatform(is: (platform: "ios" | "android") => boolean): PluginPlatform {
  if (is("ios")) return "ios";
  return is("android") ? "android" : "desktop";
}

export interface PluginHookResult {
  text?: string;
  lang?: string | null;
  auto?: { message?: boolean; composer?: boolean };
  ask?: { text: string };
  values?: Record<string, string | boolean>;
  options?: Record<string, { value: string; label: string }[]>;
}

// the core's limit too
const HOOK_LANG_LIMIT = 35;
const SETTINGS_LIMIT = 16;
const OPTIONS_LIMIT = 300;
const SETTING_TEXT_LIMIT = 200;

function settingValues(said: unknown): Record<string, string | boolean> | undefined {
  if (!said || typeof said !== "object" || Array.isArray(said)) return undefined;
  const values: Record<string, string | boolean> = {};
  for (const [key, value] of Object.entries(said).slice(0, SETTINGS_LIMIT)) {
    if (typeof value === "boolean" || (typeof value === "string" && value.length <= SETTING_TEXT_LIMIT)) values[key] = value;
  }
  return values;
}

function settingOptions(said: unknown): Record<string, { value: string; label: string }[]> | undefined {
  if (!said || typeof said !== "object" || Array.isArray(said)) return undefined;
  const options: Record<string, { value: string; label: string }[]> = {};
  for (const [key, list] of Object.entries(said).slice(0, SETTINGS_LIMIT)) {
    if (!Array.isArray(list)) continue;
    options[key] = list
      .filter((one): one is { value: string; label: string } => typeof one?.value === "string" && typeof one?.label === "string")
      .filter((one) => one.value.length <= SETTING_TEXT_LIMIT && one.label.length <= SETTING_TEXT_LIMIT)
      .slice(0, OPTIONS_LIMIT)
      .map((one) => ({ value: one.value, label: one.label }));
  }
  return options;
}

// the core's note limit too
export const HOOK_TEXT_LIMIT = 5_000;

function hookResult(said: unknown): PluginHookResult | null {
  if (!said || typeof said !== "object") return null;
  const given = said as { text?: unknown; auto?: unknown; lang?: unknown; ask?: unknown; values?: unknown; options?: unknown };
  const result: PluginHookResult = {};
  if (typeof given.text === "string") {
    if (Array.from(given.text).length > HOOK_TEXT_LIMIT) return null;
    result.text = given.text;
  }
  if (given.lang === null || (typeof given.lang === "string" && given.lang.length <= HOOK_LANG_LIMIT)) result.lang = given.lang;
  const values = settingValues(given.values);
  if (values) result.values = values;
  const options = settingOptions(given.options);
  if (options) result.options = options;
  const asked = given.ask && typeof given.ask === "object" ? (given.ask as { text?: unknown }).text : undefined;
  if (typeof asked === "string" && noticeText(asked)) result.ask = { text: noticeText(asked) };
  if (given.auto && typeof given.auto === "object") {
    const auto = given.auto as Record<string, unknown>;
    result.auto = {};
    if (typeof auto.message === "boolean") result.auto.message = auto.message;
    if (typeof auto.composer === "boolean") result.auto.composer = auto.composer;
  }
  return Object.keys(result).length ? result : null;
}

// ms
export const CLOSING_WAIT = 400;

export const NOTICE_LIMIT = 200;

export const NOTICE_DURATION = 3000;

function noticeText(said: string): string {
  // eslint-disable-next-line no-control-regex
  const clean = said.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").trim();
  const characters = Array.from(clean);
  return characters.length > NOTICE_LIMIT ? `${characters.slice(0, NOTICE_LIMIT - 1).join("").trimEnd()}…` : clean;
}

export const DRIVE_OPS = [
  "status", "connect", "setup", "unlock", "disconnect", "list", "mkdir", "rename", "move", "remove",
  "upload", "keep", "open", "save", "send", "retry", "cancel", "backup", "backupInfo", "restore",
] as const;
export type DriveOp = (typeof DRIVE_OPS)[number];

const text = (value: unknown): value is string => typeof value === "string";

// only this plugin's frame, and only what it may say
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
    case "ft.notify":
      return text(said.text) ? { type: "ft.notify", text: noticeText(said.text), sticky: said.sticky === true } : null;
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
    case "ft.translateLanguages":
      return text(id) ? { type: said.type, id } : null;
    case "ft.translateDetect":
      return text(id) && text(said.text) ? { type: "ft.translateDetect", id, text: said.text } : null;
    case "ft.translateStatus":
    case "ft.translatePrepare":
    case "ft.translateRemove":
      return text(id) && text(said.from) && text(said.to) ? { type: said.type, id, from: said.from, to: said.to } : null;
    case "ft.translateText":
      return text(id) && text(said.text) && text(said.from) && text(said.to)
        ? { type: "ft.translateText", id, text: said.text, from: said.from, to: said.to }
        : null;
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
    case "ft.hookDone":
      return text(id) ? { type: "ft.hookDone", id, result: hookResult(said.result) } : null;
    case "ft.show":
      return { type: "ft.show" };
    default:
      return null;
  }
}

export interface HandedFile {
  name: string;
  mime: string;
  data: string;
}

export function opensKind(opens: string[] | undefined, mime: string): boolean {
  const kind = mime.split(";")[0].trim().toLowerCase();
  const top = kind.split("/")[0];
  return (opens ?? []).some((one) => one === "*/*" || one === kind || one === `${top}/*`);
}

export function openersOf(plugins: PluginView[], message: { kind?: string; text?: string; file?: { mime?: string } }): PluginView[] {
  const candidates = plugins.filter((one) => !isGame(one));
  if (message.kind === "file") {
    const mime = message.file?.mime || "application/octet-stream";
    return candidates.filter((one) => opensKind(one.opens, mime));
  }
  return candidates.filter((one) => one.asks.messages && opensKind(one.opens, "text/plain"));
}

export function viewerOf(plugins: PluginView[], mime: string): PluginView | undefined {
  const kind = mime.split(";")[0].trim().toLowerCase();
  return plugins
    .filter((one) => !isGame(one) && (one.views ?? []).includes(kind))
    .sort((a, b) => b.installedAt - a.installedAt)[0];
}
