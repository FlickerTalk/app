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
export { fromFrame, type FrameMessage, type PluginHookResult } from "./frame-messages";
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

// ms
export const CLOSING_WAIT = 400;

export const NOTICE_DURATION = 3000;

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
