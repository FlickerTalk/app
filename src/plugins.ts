/**
 * Plugins in the app (issue app#3, Plan §53–§58). A plugin runs inside an iframe served from its
 * own scheme, with the policy its permissions allow, and only ever sees the text the user hands
 * it. It never touches the app's window, its storage or its keys.
 */
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

/** The first read of the list since the app started: whoever waits on it shares it. */
let firstRead: Promise<PluginView[]> | undefined;

/** Asks the core what is installed and hands it to every screen that is watching. */
export function refreshPlugins(): Promise<PluginView[]> {
  const read = plugins()
    .catch(() => [])
    .then((list) => (installed.value = list));
  firstRead ??= read;
  return read;
}

/**
 * The list once it has been read (2026-10-08, device profile): a screen that only needs it to be
 * there reads nothing when the app already did, and shares the first read while it is on its way.
 */
export function pluginsReady(): Promise<PluginView[]> {
  return firstRead ?? refreshPlugins();
}

/** What the core says when the plugins installed here changed: an update made in the background. */
export const PLUGINS_EVENT = "ft://plugins";

/**
 * Reads the plugins once, then again whenever the core says they changed (2026-10-03): an update
 * it made in the background reaches every screen. Returns how to stop listening.
 */
export async function followPluginChanges(): Promise<() => void> {
  void refreshPlugins();
  return listen(PLUGINS_EVENT, () => void refreshPlugins());
}

/**
 * Whether the premium part is locked now (decision of 2026-10-08): the tools and the extra sessions
 * with a PIN, once the 15 free days are over without a subscription. Chat, calls, files and games
 * never are. The core refuses them by itself; this only lets the screens show the lock and offer
 * the subscription instead.
 */
export const premiumLocked = shallowRef(false);

/** Asks the core where the plan stands. A core that cannot answer locks nothing. */
export async function refreshPremiumLock(): Promise<boolean> {
  premiumLocked.value = (await plan().catch(() => null))?.state === "limited";
  return premiumLocked.value;
}

/** Reads the lock again whenever the core says the plan changed. Returns how to stop listening. */
export async function followPremiumLock(): Promise<() => void> {
  void refreshPremiumLock();
  return listen(PLAN_EVENT, () => void refreshPremiumLock());
}

/** Whether this plugin is locked now: a tool while the tools are; a game, never. */
export function isLocked(plugin: { kind?: "tool" | "game" }): boolean {
  return premiumLocked.value && !isGame(plugin);
}

/**
 * The tools of the chat and the games, apart (plan 10.3): the tools sheet, "open with" and
 * Settings show tools only; games have their own section. Anything not marked a game is a tool.
 */
export const tools = computed(() => byPluginName(installed.value.filter((one) => !isGame(one))));
export const games = computed(() => byPluginName(installed.value.filter(isGame)));

/**
 * The Ionicons a plugin may name for its tile (2026-10-08, plan of the apps grid), by their
 * kebab-case name as `module.json` says it. A whitelist, not the whole set: the app carries only
 * these, and a name it does not know draws the app's own icon. It holds every icon chosen for our
 * plugins and every icon a plugin's frame can ask the core for (`src-tauri/resources/icons`).
 */
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

/**
 * The icon of a plugin's tile, as the SVG data URL `ion-icon` takes: the Ionicon its manifest
 * names, if the app allows it; otherwise a puzzle piece for a tool and a controller for a game.
 */
export function pluginIcon(plugin: { icon?: string; kind?: "tool" | "game" }): string {
  // The map has no prototype: `toString` or `__proto__` is no icon.
  const named = plugin.icon && plugin.icon in PLUGIN_ICONS ? PLUGIN_ICONS[plugin.icon] : undefined;
  return named ?? (isGame(plugin) ? gameControllerOutline : extensionPuzzleOutline);
}

/**
 * The plugin's own image for its tile (2026-10-08, "Imagen por plugin"): its `icon.svg`, which
 * the core checked, as a base64 data URL for an `<img>`, where a browser runs nothing. None when
 * the package carries no image: the tile draws `pluginIcon` instead.
 */
export function pluginImage(plugin: { image?: string }): string | undefined {
  if (!plugin.image) return undefined;
  const bytes = new TextEncoder().encode(plugin.image);
  let binary = "";
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return `data:image/svg+xml;base64,${btoa(binary)}`;
}

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

// ---- The chat's hooks (2026-10-20, plan of the translator §8) ----

/** The tools of this phone bound to a hook of the chat, by the name the phone shows. Never a game. */
export function hooked(name: PluginHookName): PluginView[] {
  return tools.value.filter((one) => Boolean(one.hooks?.[name]));
}

/**
 * What a tool says of one of its hooks in the phone's language (`locales.<lang>.hooks`), as for the
 * name: the exact language, then its base; otherwise `english`. `pick` reads it from the hook's
 * labels in a language: a label, or the settings hook's object.
 */
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

/**
 * A hook's label in the phone's language, else the English one; for the settings hook, its own
 * label. Empty for a hook the tool does not bind to.
 */
export function hookLabel(plugin: PluginView, name: PluginHookName): string {
  const english = plugin.hooks?.[name]?.label ?? "";
  if (!english) return "";
  return hookSaid(plugin, name, english, (said) => (typeof said === "string" ? said : said.label));
}

/**
 * The settings the app draws for a tool in a conversation's page (§8.2d), in order, each labelled in
 * the phone's language (else in English). None for a tool without the settings hook.
 */
export function settingsFields(plugin: PluginView): PluginSettingsField[] {
  return (plugin.hooks?.settings?.fields ?? []).map((field) => ({
    ...field,
    label: hookSaid(plugin, "settings", field.label, (said) => (typeof said === "string" ? undefined : said.fields?.[field.key])),
  }));
}

/** The line under a tool's settings, in the phone's language (else in English); "" for none. */
export function settingsNote(plugin: PluginView): string {
  const english = plugin.hooks?.settings?.note ?? "";
  if (!english) return "";
  return hookSaid(plugin, "settings", english, (said) => (typeof said === "string" ? undefined : said.note));
}

/** A hook's icon: the Ionicon it names, if the app carries it; otherwise the tool's puzzle piece. */
export function hookIcon(plugin: PluginView, name: PluginHookName): string {
  return pluginIcon({ icon: plugin.hooks?.[name]?.icon, kind: "tool" });
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
  /** 2026-10-06: a notice the app shows as its single toast; an empty text clears the current one. */
  | { type: "ft.notify"; text: string; sticky: boolean }
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
  // 2026-10-20 (app#177): the phone's own translator.
  | { type: "ft.translateLanguages"; id: string }
  | { type: "ft.translateDetect"; id: string; text: string }
  | { type: "ft.translateStatus" | "ft.translatePrepare" | "ft.translateRemove"; id: string; from: string; to: string }
  | { type: "ft.translateText"; id: string; text: string; from: string; to: string }
  // The user's cloud (plan-drive): one question with an operation and up to two strings.
  | { type: "ft.drive"; id: string; op: DriveOp; a: string; b: string }
  // 2026-10-20 (plan of the translator §8): a hook's answer, and a plugin out of sight asking to be seen.
  | { type: "ft.hookDone"; id: string; result: PluginHookResult | null }
  | { type: "ft.show" };

/** What the app hands a plugin for one run of a hook (`ft.hook`); `chat` is `pluginChat`'s key. */
export interface PluginHook {
  name: PluginHookName;
  /** The message or the draft; "" for the chat hook. */
  text: string;
  chat: string;
  /** The message's ref (`message` only). */
  ref: string | null;
  lang: string;
  /** Started by the user, not by the conversation's own switches. */
  manual: boolean;
  /** The user said yes to the question the tool asked in its last answer (§8.2c): run it again. */
  answer?: boolean;
  /** `settings` run by the user (§8.2d): the values changed, by field key. */
  values?: Record<string, string | boolean>;
}

/**
 * A hook's answer: a text (a note, a new draft), the chat's language as the tool found it, the chat
 * hook's two switches, or a question for the user (a download) that the app asks in its own alert,
 * running the hook again with `answer` on a yes (§8.2c).
 */
export interface PluginHookResult {
  text?: string;
  /** The conversation's language (`settings`); `null` or "" for none. */
  lang?: string | null;
  auto?: { message?: boolean; composer?: boolean };
  ask?: { text: string };
  /** The settings' values by field key (§8.2d): what each row shows. */
  values?: Record<string, string | boolean>;
  /** The choices of each select by field key, in order, labelled in the app's language. */
  options?: Record<string, { value: string; label: string }[]>;
}

/** The longest language tag a hook may say (the core's limit too). */
const HOOK_LANG_LIMIT = 35;
/** The most settings, choices of a select, and characters of a value or a label a hook may give. */
const SETTINGS_LIMIT = 16;
const OPTIONS_LIMIT = 300;
const SETTING_TEXT_LIMIT = 200;

/** Settings' values by key: strings (cut to a sensible length) and yes-or-noes, nothing else. */
function settingValues(said: unknown): Record<string, string | boolean> | undefined {
  if (!said || typeof said !== "object" || Array.isArray(said)) return undefined;
  const values: Record<string, string | boolean> = {};
  for (const [key, value] of Object.entries(said).slice(0, SETTINGS_LIMIT)) {
    if (typeof value === "boolean" || (typeof value === "string" && value.length <= SETTING_TEXT_LIMIT)) values[key] = value;
  }
  return values;
}

/** The choices of each select by key: a value and a label each, as many as there are languages. */
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

/** The longest text a hook may hand back: as long as a message (the core's note limit too). */
export const HOOK_TEXT_LIMIT = 5_000;

/** Only what a hook's answer can say; anything else, or a text longer than a message, is none. */
function hookResult(said: unknown): PluginHookResult | null {
  if (!said || typeof said !== "object") return null;
  const given = said as { text?: unknown; auto?: unknown; lang?: unknown; ask?: unknown; values?: unknown; options?: unknown };
  const result: PluginHookResult = {};
  if (typeof given.text === "string") {
    if (Array.from(given.text).length > HOOK_TEXT_LIMIT) return null;
    result.text = given.text;
  }
  // `null` (or an empty one): the conversation has no language any more (the user chose none).
  if (given.lang === null || (typeof given.lang === "string" && given.lang.length <= HOOK_LANG_LIMIT)) result.lang = given.lang;
  const values = settingValues(given.values);
  if (values) result.values = values;
  const options = settingOptions(given.options);
  if (options) result.options = options;
  const asked = given.ask && typeof given.ask === "object" ? (given.ask as { text?: unknown }).text : undefined;
  // Shown in the app's own alert: cleaned and cut as a notice is.
  if (typeof asked === "string" && noticeText(asked)) result.ask = { text: noticeText(asked) };
  if (given.auto && typeof given.auto === "object") {
    const auto = given.auto as Record<string, unknown>;
    result.auto = {};
    if (typeof auto.message === "boolean") result.auto.message = auto.message;
    if (typeof auto.composer === "boolean") result.auto.composer = auto.composer;
  }
  return Object.keys(result).length ? result : null;
}

/**
 * How long a closing plugin is kept, out of sight, to say goodbye to its twin (2026-10-02): its
 * window goes at once, and the frame goes when the plugin answers `ft.closed` or after this, in ms.
 */
export const CLOSING_WAIT = 400;

/** The longest notice a plugin can show, in characters, the ellipsis included (2026-10-06). */
export const NOTICE_LIMIT = 200;

/** How long a notice that is not sticky stays on screen, in ms (2026-10-06). */
export const NOTICE_DURATION = 3000;

/**
 * A plugin's notice as the app shows it: control characters (line breaks included) become spaces,
 * the ends are trimmed and a long text is cut, with an ellipsis, so no plugin fills the screen.
 */
function noticeText(said: string): string {
  // eslint-disable-next-line no-control-regex
  const clean = said.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").trim();
  const characters = Array.from(clean);
  return characters.length > NOTICE_LIMIT ? `${characters.slice(0, NOTICE_LIMIT - 1).join("").trimEnd()}…` : clean;
}

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
 * and ask to read what they are handed; a file, to those that open its kind. Asking is enough to
 * be offered (decision of 2026-10-20, app#177): the permission is asked for when the user picks
 * one, so a tool just installed is found where it is needed.
 */
export function openersOf(plugins: PluginView[], message: { kind?: string; text?: string; file?: { mime?: string } }): PluginView[] {
  const candidates = plugins.filter((one) => !isGame(one));
  if (message.kind === "file") {
    const mime = message.file?.mime || "application/octet-stream";
    return candidates.filter((one) => opensKind(one.opens, mime));
  }
  return candidates.filter((one) => one.asks.messages && opensKind(one.opens, "text/plain"));
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
