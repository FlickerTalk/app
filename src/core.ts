/**
 * Bridge to the Rust core (Plan §82, §106 M3): the UI shows what the core has and forwards the
 * user's intents. Keys, the route capability and the network never reach the WebView (§54).
 */
import { reactive } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

export type Status = "pending" | "sent" | "delivered" | "read";

/** `paused`: the transfer cannot move without a direct connection (§62). */
export type FileState = "sending" | "receiving" | "paused" | "done" | "failed";

export interface ChatFile {
  name: string;
  size: string;
  mime: string;
  progress: number;
  state: FileState;
  /** For images on this device: the sender's copy, or the receiver's once complete. */
  url?: string;
}

export interface ChatMessage {
  id: string;
  mine: boolean;
  text: string;
  time: string;
  status?: Status;
  kind?: "file";
  file?: ChatFile;
}

export interface Chat {
  id: string;
  name: string;
  hue: number;
  /** Whether a direct connection is open now. */
  connected: boolean;
  unread: number;
  time: string;
  preview: string;
  lastMine: boolean;
  /** What the last message was, for the list. */
  lastKind: "text" | "file" | "voice";
  status: Status | "";
  blocked: boolean;
  messages: ChatMessage[];
}

export interface Me {
  id: string;
  name: string;
  hue: number;
  mailbox: boolean;
  /** Whether contacts added from now on are told their messages arrived and were read (app#6). */
  receipts: boolean;
  /** Until when (ms) the app is free: a year from the install, counted on this phone (§41). */
  freeUntil: number;
}

/** What this phone takes from a contact and tells them (issues app#4–#6). */
export interface ContactRules {
  /** Their calls show but neither ring nor vibrate. */
  muted: boolean;
  acceptsChat: boolean;
  acceptsCalls: boolean;
  /** They see their messages as delivered and read. */
  receipts: boolean;
}

/** One day of the weekly hours (app#7): all day, never, or a stretch "HH:MM"–"HH:MM". */
export type Day = "all" | "none" | { from: string; to: string };

/** The weekly hours when the phone may make noise, Monday first. */
export interface Week {
  days: Day[];
}

export interface ContactDetails {
  id: string;
  name: string;
  fingerprint: string;
  mailbox: boolean;
  blocked: boolean;
  /** Seconds this phone keeps their messages; 0 forever (issue app#1). */
  keepFor: number;
  /** Seconds a read message stays after being read; 0 never. */
  burnAfterRead: number;
  rules: ContactRules;
}

interface FileView {
  name: string;
  size: number;
  mime: string;
  progress: number;
  state: "transferring" | "done" | "failed";
  path: string;
}

interface MessageView {
  id: string;
  outgoing: boolean;
  text: string;
  sentAt: number;
  state: Status;
  file?: FileView;
}

interface ConversationView {
  id: string;
  name: string;
  unread: number;
  blocked: boolean;
  connected: boolean;
  last: MessageView | null;
}

/**
 * A hidden session: its own contacts and conversations, opened with a 6-digit PIN and nothing
 * else, not even a name. Only the person who created it knows it exists; while it is closed it
 * receives texts and files in silence, with no notification and no calls.
 */
export interface Session {
  id: string;
  chats: Chat[];
}

interface SessionView {
  id: string;
  conversations: ConversationView[];
}

export const CHANGED_EVENT = "ft://changed";
const MESSAGE_LIMIT = 200;
/** Bytes per call when copying a picked file into the app. */
const UPLOAD_SLICE = 512 * 1024;

export const store = reactive({
  ready: false,
  me: { id: "", name: "", hue: 0, mailbox: true, receipts: true, freeUntil: 0 } as Me,
  chats: [] as Chat[],
  /** The hidden sessions open right now; an empty list looks exactly like having none. */
  sessions: [] as Session[],
});

/** Conversations whose messages are shown, so a change reloads them. */
const loaded = new Set<string>();

/** A stable colour per contact, from its id. */
export function hueOf(id: string): number {
  let hash = 2166136261;
  for (const char of id) {
    hash = Math.imul(hash ^ char.charCodeAt(0), 16777619);
  }
  return (hash >>> 0) % 360;
}

export function clock(ms: number): string {
  if (!ms) return "";
  const date = new Date(ms);
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Sizes as people read them: 512 B, 1.5 KB, 48 MB. */
export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const shown = unit === 0 || value >= 10 ? Math.round(value).toString() : value.toFixed(1).replace(/\.0$/, "");
  return `${shown} ${units[unit]}`;
}

function toFile(view: FileView, mine: boolean, connected: boolean): ChatFile {
  let state: FileState;
  if (view.state === "done" || view.state === "failed") state = view.state;
  else if (!connected) state = "paused";
  else state = mine ? "sending" : "receiving";
  const playable = view.mime.startsWith("image/") || view.mime.startsWith("audio/");
  const showable = playable && (mine || view.state === "done");
  return {
    name: view.name,
    size: formatSize(view.size),
    mime: view.mime,
    progress: view.progress,
    state,
    url: showable ? convertFileSrc(view.path) : undefined,
  };
}

function toMessage(view: MessageView, connected = false): ChatMessage {
  const message: ChatMessage = { id: view.id, mine: view.outgoing, text: view.text, time: clock(view.sentAt), status: view.state };
  if (view.file) {
    message.kind = "file";
    message.file = toFile(view.file, view.outgoing, connected);
  }
  return message;
}

function toChat(view: ConversationView): Chat {
  return {
    id: view.id,
    name: view.name,
    hue: hueOf(view.id),
    connected: view.connected,
    unread: view.unread,
    time: clock(view.last?.sentAt ?? 0),
    preview: view.last?.text ?? "",
    lastMine: view.last?.outgoing ?? false,
    lastKind: !view.last?.file ? "text" : view.last.file.mime.startsWith("audio/") ? "voice" : "file",
    status: view.last?.state ?? "",
    blocked: view.blocked,
    messages: chat(view.id)?.messages ?? [],
  };
}

export function chat(id: string): Chat | undefined {
  return allChats().find((candidate) => candidate.id === id);
}

/** The main list plus every open session's conversations. */
function allChats(): Chat[] {
  return store.chats.concat(...store.sessions.map((session) => session.chats));
}

/**
 * Opens the session that has this PIN or, if none has it, a new empty one. The core never says
 * which of the two happened, so nothing reveals whether a session existed.
 */
export async function openSession(pin: string): Promise<void> {
  const view = await invoke<SessionView>("core_session_open", { pin });
  const session: Session = { id: view.id, chats: view.conversations.map(toChat) };
  const index = store.sessions.findIndex((candidate) => candidate.id === session.id);
  if (index >= 0) store.sessions[index] = session;
  else store.sessions.push(session);
}

/** Leaves the session: it disappears from the screen and keeps receiving in silence. */
export async function closeSession(session: string): Promise<void> {
  await invoke("core_session_close", { session });
  store.sessions = store.sessions.filter((candidate) => candidate.id !== session);
}

export async function start(): Promise<void> {
  const me = await invoke<Omit<Me, "hue">>("core_me");
  store.me = { ...me, hue: hueOf(me.id) };
  await refreshChats();
  await listen<{ contact: string | null }>(CHANGED_EVENT, ({ payload }) => {
    // The list first: an open conversation's files depend on the connection it reports.
    void refreshChats().then(() => {
      if (payload.contact && loaded.has(payload.contact)) {
        return loadMessages(payload.contact);
      }
    });
  });
  store.ready = true;
}

export async function refreshChats(): Promise<void> {
  const views = await invoke<ConversationView[]>("core_conversations", undefined);
  store.chats = views.map(toChat);
  // The open sessions follow: their unread counts change with the same events.
  const sessions = (await invoke<SessionView[] | undefined>("core_sessions", undefined)) ?? [];
  store.sessions = sessions.map((session) => ({ id: session.id, chats: session.conversations.map(toChat) }));
}

export async function loadMessages(contact: string): Promise<void> {
  loaded.add(contact);
  const views = await invoke<MessageView[]>("core_messages", { contact, limit: MESSAGE_LIMIT });
  const target = chat(contact);
  if (target) {
    target.messages = views.map((view) => toMessage(view, target.connected));
  }
}

export async function sendText(contact: string, text: string): Promise<void> {
  await invoke("core_send", { contact, text });
}

async function toBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

/** A file the user picked with the phone's own picker, already in the app's folder. */
export interface PickedFile {
  path: string;
  name: string;
  mime: string;
  size: number;
}

/**
 * The phone's file picker. On Android the WebView's own file input opens a screen the user cannot
 * come back from without picking something, so the app asks the system itself (§62).
 */
export async function pickFiles(accept = ""): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_pick_files", { accept });
}

/** A photo taken right now with the phone's camera app, waiting in the app's folder like a
 * picked file; nothing if the user backed out. */
export async function takePhoto(): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_take_photo");
}

/** The bytes of a file the user picked, for a plugin that asked for one (issue app#3). */
export async function readPicked(path: string): Promise<string> {
  return invoke<string>("core_read_picked", { path });
}

/** Sends what a plugin made: the app writes the bytes and sends them as a file. */
export async function sendMade(contact: string, name: string, mime: string, data: string): Promise<void> {
  await invoke("core_send_made", { contact, name, mime, data });
}

/** Sends a picked file: its bytes never travel through the WebView. */
export async function sendPicked(contact: string, file: PickedFile): Promise<void> {
  await invoke("core_send_picked", { contact, file });
}

/**
 * Copies the picked file into the app, a slice at a time, and offers it to the contact. The
 * slices travel as base64: on Android the IPC only carries JSON.
 */
export async function sendFile(contact: string, file: File): Promise<void> {
  const upload = await invoke<string>("core_upload_start");
  for (let offset = 0; offset < file.size; offset += UPLOAD_SLICE) {
    const data = await toBase64(file.slice(offset, offset + UPLOAD_SLICE));
    await invoke("core_upload_append", { upload, data });
  }
  await invoke("core_send_file", { contact, upload, name: file.name, mime: file.type || "application/octet-stream" });
}

/**
 * Lets the router wake this phone when the app is closed (M4): notification permission and the
 * push token. Where there is no push yet (iOS), nothing happens.
 */
export async function enablePush(): Promise<void> {
  await invoke("core_enable_push").catch(() => undefined);
}

/** Shows a file in the viewer the user picks. */
export async function openFile(message: string): Promise<void> {
  await invoke("core_open_file", { message });
}

/** Copies a file to the phone's Downloads. */
export async function saveFile(message: string): Promise<void> {
  await invoke("core_save_file", { message });
}

export async function markRead(contact: string): Promise<void> {
  await invoke("core_mark_read", { contact });
}

/** This device's Contact Card as a link, or a hidden session's own (app#9): shown as a QR code and shared. */
export async function myCardLink(session?: string): Promise<string> {
  return invoke<string>("core_card", session ? { session } : undefined);
}

/**
 * Takes this device off the router and wipes everything FlickerTalk keeps on the phone (§78).
 * The app starts again empty; there is no way back.
 */
export async function erasePhone(): Promise<void> {
  await invoke("core_erase");
}

/** Opens the phone's share sheet with `text`; fails where there is none (a desktop). */
export async function shareText(text: string): Promise<void> {
  await invoke("core_share", { text });
}

/** Adds the owner of a scanned or pasted card, to a hidden session if given, and returns their id. */
export async function addContact(link: string, session?: string): Promise<string> {
  return invoke<string>("core_add_contact", { link: link.trim(), session });
}

export async function setName(name: string): Promise<void> {
  const trimmed = name.trim();
  await invoke("core_set_name", { name: trimmed });
  store.me.name = trimmed;
}

export async function setMailbox(enabled: boolean): Promise<void> {
  await invoke("core_set_mailbox", { enabled });
  store.me.mailbox = enabled;
}

export async function setRules(contact: string, rules: ContactRules): Promise<void> {
  await invoke("core_set_rules", { contact, rules });
}

/** Whether contacts added from now on get receipts; each contact's page can change its own. */
export async function setReceipts(enabled: boolean): Promise<void> {
  await invoke("core_set_receipts", { enabled });
  store.me.receipts = enabled;
}

/** The weekly hours, or `null` when they are off. */
export async function quietHours(): Promise<Week | null> {
  const json = await invoke<string | null>("core_quiet_hours");
  return json ? (JSON.parse(json) as Week) : null;
}

export async function setQuietHours(week: Week | null): Promise<void> {
  await invoke("core_set_quiet_hours", { hours: week ? JSON.stringify(week) : null });
}

/** Where reports go: email, outside the messaging system (§36). */
const REPORT_ADDRESS = "info@flickertalk.com";

/** A report as an email: who, why and, only if the user chose it, their messages as evidence. */
export function reportLink(contact: string, reason: string, evidence: string[]): string {
  const lines = [`Reported: ${contact}`, `Reason: ${reason}`];
  if (evidence.length) lines.push("", "Evidence:", ...evidence.map((text) => `> ${text}`));
  const subject = encodeURIComponent(`Report ${contact}`);
  return `mailto:${REPORT_ADDRESS}?subject=${subject}&body=${encodeURIComponent(lines.join("\n"))}`;
}

/** A plugin on this phone, with what it asks for and what it may do (issue app#3). */
export interface PluginPermissions {
  /** Hosts it may talk to; empty is no network at all. */
  network: string[];
  /** Whether it may read the message the user hands it. */
  messages: boolean;
  /** "nothing", "propose" (fills the composer) or "auto" (sends by itself). */
  send: string;
}

export interface PluginView {
  id: string;
  name: string;
  version: string;
  asks: PluginPermissions;
  granted: PluginPermissions;
  installedAt: number;
}

export async function plugins(): Promise<PluginView[]> {
  return invoke<PluginView[]>("core_plugins");
}

/** A tool the app carries; it is only installed if the user says so (§53). */
export interface OfferedPlugin {
  id: string;
  name: string;
  version: string;
  summary: string;
  size: number;
  installed: boolean;
  /** Whether the app already carries it; if not, adding it downloads it (§52). */
  carried: boolean;
}

export async function offeredPlugins(): Promise<OfferedPlugin[]> {
  return invoke<OfferedPlugin[]>("core_catalogue");
}

export async function installPlugin(plugin: string): Promise<void> {
  await invoke("core_plugin_add", { plugin });
}

/** Where this phone stands with the plan (§40–§47); all of it decided on the phone. */
export interface PlanView {
  state: "trial" | "young" | "subscribed" | "limited";
  /** When the free year ends, or when the subscription runs out (ms); 0 when neither applies. */
  until: number;
  age: "minor" | "adult" | "unknown";
}

export async function plan(): Promise<PlanView> {
  return invoke<PlanView>("core_plan");
}

/** What the user says about their age; never a date of birth (§30, §43). */
export async function setAge(age: "minor" | "adult"): Promise<void> {
  await invoke("core_set_age", { age });
}

/** Asks the Store for the subscription. No payment data ever reaches us (§47). */
export async function subscribe(): Promise<void> {
  await invoke("core_subscribe");
}

/** What the user does with one message of theirs (§61). */
export async function forgetMessage(message: string): Promise<void> {
  await invoke("core_forget_message", { message });
}

export async function forwardMessage(message: string, contact: string): Promise<void> {
  await invoke("core_forward", { message, contact });
}

export async function shareMessage(message: string): Promise<void> {
  await invoke("core_share_message", { message });
}

/** What the core does for a plugin, and only after checking what the user granted it (§53). */
export async function pluginSave(name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_save", { name, mime, data });
}

export async function pluginPrint(plugin: string, name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_print", { plugin, name, mime, data });
}

export async function pluginFetch(
  plugin: string,
  url: string,
  method: string,
  headers: [string, string][],
  body: string | null,
): Promise<{ status: number; body: string }> {
  return invoke("core_plugin_fetch", { plugin, url, method, headers, body });
}

export async function pluginRead(plugin: string, key: string): Promise<string | null> {
  return invoke<string | null>("core_plugin_read", { plugin, key });
}

export async function pluginWrite(plugin: string, key: string, value: string): Promise<void> {
  await invoke("core_plugin_write", { plugin, key, value });
}

export async function pluginForget(plugin: string, key: string): Promise<void> {
  await invoke("core_plugin_forget", { plugin, key });
}

/** What the user allows a plugin to do; never more than it asked for (§53). */
export async function grantPlugin(plugin: string, granted: PluginPermissions): Promise<void> {
  await invoke("core_plugin_grant", { plugin, granted });
}

export async function removePlugin(plugin: string): Promise<void> {
  await invoke("core_plugin_remove", { plugin });
}

/** The name this phone shows for a contact; it never leaves the phone (issue app#1). */
export async function renameContact(contact: string, name: string): Promise<void> {
  await invoke("core_rename", { contact, name: name.trim() });
  const chat = store.chats.find((item) => item.id === contact);
  if (chat) chat.name = name.trim();
}

/**
 * How long this phone keeps the conversation with a contact, and how long a read message stays
 * after being read, in seconds; 0 means forever and never (issue app#1).
 */
export async function setHistory(contact: string, keepFor: number, burnAfterRead: number): Promise<void> {
  await invoke("core_set_history", { contact, keepFor, burnAfterRead });
}

/** Opens the report in the user's mail app and blocks the contact. */
export async function reportContact(contact: string, reason: string, evidence: string[]): Promise<void> {
  await openUrl(reportLink(contact, reason, evidence));
  await block(contact, true);
}

export async function contactDetails(contact: string): Promise<ContactDetails> {
  return invoke<ContactDetails>("core_contact", { contact });
}

export async function block(contact: string, blocked: boolean): Promise<void> {
  await invoke("core_block", { contact, blocked });
  await refreshChats();
}
