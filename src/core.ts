/**
 * Bridge to the Rust core (Plan §82, §106 M3): the UI shows what the core has and forwards the
 * user's intents. Keys, the route capability and the network never reach the WebView (§54).
 */
import { reactive } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";

export type Status = "unsent" | "pending" | "sent" | "delivered" | "read";

/**
 * `paused`: the transfer cannot move without a direct connection (§62). `waiting`: bigger than
 * what this phone downloads on its own; it waits for a tap (A4).
 */
export type FileState = "sending" | "receiving" | "paused" | "waiting" | "done" | "failed";

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

/** Someone in a circle, as this phone names them. */
export interface CircleMember {
  id: string;
  name: string;
  admin: boolean;
  /** Whether it is this phone. */
  me: boolean;
}

/** What a circle message is: a text, or something that happened, named in `text`. */
export type CircleKind = "text" | "created" | "joined" | "left" | "removed" | "renamed";

export interface CircleMessage {
  id: string;
  mine: boolean;
  text: string;
  time: string;
  status?: Status;
  kind: CircleKind;
  sender: string;
  senderName: string;
}

/**
 * A circle (2026-09-27): a small closed group of contacts, no server knowing it exists. Every
 * message goes to each member through the channel this phone already has with them.
 */
export interface Circle {
  id: string;
  name: string;
  hue: number;
  members: CircleMember[];
  /** Whether this phone may change it now. */
  admin: boolean;
  adminsOnly: boolean;
  /** This phone left, or was taken out: what was said stays, read only. */
  left: boolean;
  unread: number;
  time: string;
  preview: string;
  lastMine: boolean;
  /** Who said the last thing, when it was a text of someone else's; empty otherwise. */
  lastSender: string;
  status: Status | "";
  messages: CircleMessage[];
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
  /** Files up to this many bytes are downloaded as they arrive; bigger ones wait (A4). */
  autoDownload: number;
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
  state: "transferring" | "waiting" | "done" | "failed";
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

interface CircleMessageView {
  id: string;
  outgoing: boolean;
  sender: string;
  senderName: string;
  kind: CircleKind;
  text: string;
  sentAt: number;
  state: Status;
}

interface CircleView {
  id: string;
  name: string;
  members: CircleMember[];
  admin: boolean;
  adminsOnly: boolean;
  left: boolean;
  unread: number;
  last: CircleMessageView | null;
}

/**
 * A hidden session: its own contacts and conversations, opened with a 6-digit PIN and nothing
 * else, not even a name. Only the person who created it knows it exists; while it is closed it
 * receives texts and files in silence, with no notification and no calls.
 */
export interface Session {
  /** Empty for a session that is only on the screen: no room for another, shown all the same (A3). */
  id: string;
  chats: Chat[];
  /** Strangers who wrote to this session with its link and wait for a yes (A5). */
  requests: Chat[];
  /** The circles made in this session, of its contacts. */
  circles: Circle[];
  /** Only for a session that is only on the screen: its PIN, kept in memory, to tell it apart. */
  pin?: string;
}

interface SessionView {
  id: string;
  conversations: ConversationView[];
  requests: ConversationView[];
  circles?: CircleView[];
}

export const CHANGED_EVENT = "ft://changed";
const MESSAGE_LIMIT = 200;
/** Bytes per call when copying a picked file into the app. */
const UPLOAD_SLICE = 512 * 1024;

export const store = reactive({
  ready: false,
  me: { id: "", name: "", hue: 0, mailbox: true, receipts: true, freeUntil: 0, autoDownload: 0 } as Me,
  chats: [] as Chat[],
  /** Strangers who wrote first with this phone's link and wait for a yes (A5). */
  requests: [] as Chat[],
  /** The hidden sessions open right now; an empty list looks exactly like having none. */
  sessions: [] as Session[],
  /** The circles of the main list (2026-09-27). */
  circles: [] as Circle[],
});

/** Conversations whose messages are shown, so a change reloads them. */
const loaded = new Set<string>();
/** Circles whose messages are shown. */
const loadedCircles = new Set<string>();

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
  if (view.state === "done" || view.state === "failed" || view.state === "waiting") state = view.state;
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

function toCircleMessage(view: CircleMessageView): CircleMessage {
  return {
    id: view.id,
    mine: view.outgoing,
    text: view.text,
    time: clock(view.sentAt),
    status: view.state,
    kind: view.kind,
    sender: view.sender,
    senderName: view.senderName,
  };
}

function toCircle(view: CircleView): Circle {
  return {
    id: view.id,
    name: view.name,
    hue: hueOf(view.id),
    members: view.members,
    admin: view.admin,
    adminsOnly: view.adminsOnly,
    left: view.left,
    unread: view.unread,
    time: clock(view.last?.sentAt ?? 0),
    preview: view.last?.text ?? "",
    lastMine: view.last?.outgoing ?? false,
    lastSender: view.last && !view.last.outgoing && view.last.kind === "text" ? view.last.senderName : "",
    status: view.last?.state ?? "",
    messages: circle(view.id)?.messages ?? [],
  };
}

/**
 * The contacts a circle may take: those of the list it lives in, the main one or an open
 * session's, and the session itself when it is one.
 */
export function circleHome(id: string): { session?: string; contacts: Chat[] } {
  for (const session of store.sessions) {
    if (session.circles.some((candidate) => candidate.id === id)) return { session: session.id, contacts: session.chats };
  }
  return { contacts: store.chats };
}

/**
 * The open hidden session a conversation (or a stranger's request) lives in; undefined for the
 * main list. A plugin opened from that conversation is open there (2026-10-01, §108).
 */
export function sessionOf(contact: string): string | undefined {
  if (!contact) return undefined;
  return store.sessions.find((session) => session.chats.concat(session.requests).some((one) => one.id === contact))?.id || undefined;
}

/** A circle of the main list or of any open session. */
export function circle(id: string): Circle | undefined {
  return store.circles.concat(...store.sessions.map((session) => session.circles)).find((candidate) => candidate.id === id);
}

/** The main list, the requests and every open session's conversations. */
function allChats(): Chat[] {
  return store.chats.concat(store.requests, ...store.sessions.flatMap((session) => session.chats.concat(session.requests)));
}

function toSession(view: SessionView): Session {
  return {
    id: view.id,
    chats: view.conversations.map(toChat),
    requests: view.requests.map(toChat),
    circles: (view.circles ?? []).map(toCircle),
  };
}

function showSession(session: Session) {
  const index = store.sessions.findIndex((candidate) => candidate.id === session.id && candidate.pin === session.pin);
  if (index >= 0) store.sessions[index] = session;
  else store.sessions.push(session);
}

/**
 * Opens the session that has this PIN or, if none has it, a new empty one (A3): every PIN is
 * valid, and nothing reveals whether a session existed. Only with the seven slots taken does the
 * core open nothing; the screen shows an empty session all the same.
 */
export async function openSession(pin: string): Promise<void> {
  const view = await invoke<SessionView | null>("core_session_open", { pin });
  showSession(view ? toSession(view) : { id: "", pin, chats: [], requests: [], circles: [] });
}

/**
 * Leaves the session: it disappears from the screen and keeps receiving in silence. One with
 * nobody in it goes for good, in the core (A3).
 */
export async function closeSession(session: string): Promise<void> {
  if (session) await invoke("core_session_close", { session });
  store.sessions = store.sessions.filter((candidate) => candidate.id !== session);
}

/** Takes a session away for good, with its contacts and history (A3). */
export async function removeSession(session: string): Promise<void> {
  await invoke("core_session_remove", { session });
  store.sessions = store.sessions.filter((candidate) => candidate.id !== session);
}

/** Yes to a stranger who wrote first (A5): they join the list. */
export async function acceptContact(contact: string): Promise<void> {
  await invoke("core_accept_contact", { contact });
  await refreshChats();
}

/** No to a stranger (A5): blocked, and out of the requests. */
export async function declineContact(contact: string): Promise<void> {
  await invoke("core_decline_contact", { contact });
  await refreshChats();
}

/**
 * Retires this phone's link and makes a new one (A5): whoever kept the old one can no longer
 * reach it. Of the main list, or of an open session. Returns the new link.
 */
export async function renewLink(session?: string): Promise<string> {
  return invoke<string>("core_renew_link", session ? { session } : {});
}

/** Sends again a message the router refused (§84): the same message, queued again. */
export async function resend(message: string): Promise<void> {
  await invoke("core_resend", { message });
}

/** The user asks for a file that was waiting for them (A4). */
export async function acceptFile(message: string): Promise<void> {
  await invoke("core_accept_file", { message });
}

/** Files up to this many bytes come on their own; 0 means always ask (A4). */
export async function setAutoDownload(bytes: number): Promise<void> {
  await invoke("core_set_auto_download", { bytes });
  store.me.autoDownload = bytes;
}

export async function start(): Promise<void> {
  const me = await invoke<Omit<Me, "hue">>("core_me");
  store.me = { ...me, hue: hueOf(me.id) };
  await refreshChats();
  await listen<{ contact: string | null; circle?: string | null }>(CHANGED_EVENT, ({ payload }) => {
    // The list first: an open conversation's files depend on the connection it reports.
    void refreshChats().then(() => {
      if (payload.contact && loaded.has(payload.contact)) {
        return loadMessages(payload.contact);
      }
      if (payload.circle && loadedCircles.has(payload.circle)) {
        return loadCircleMessages(payload.circle);
      }
    });
  });
  store.ready = true;
}

export async function refreshChats(): Promise<void> {
  const views = await invoke<ConversationView[]>("core_conversations", undefined);
  store.chats = views.map(toChat);
  const requests = (await invoke<ConversationView[] | undefined>("core_requests", {})) ?? [];
  store.requests = requests.map(toChat);
  const circles = (await invoke<CircleView[] | undefined>("core_circles", undefined)) ?? [];
  store.circles = circles.map(toCircle);
  // The open sessions follow: their unread counts change with the same events. A session that
  // is only on the screen (A3) stays as it is.
  const sessions = (await invoke<SessionView[] | undefined>("core_sessions", undefined)) ?? [];
  const shown = store.sessions.filter((session) => session.id === "");
  store.sessions = sessions.map(toSession).concat(shown);
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

// ---- Circles (2026-09-27) ----

export async function loadCircleMessages(id: string): Promise<void> {
  loadedCircles.add(id);
  const views = await invoke<CircleMessageView[]>("core_circle_messages", { circle: id, limit: MESSAGE_LIMIT });
  const target = circle(id);
  if (target) target.messages = views.map(toCircleMessage);
}

export async function sendCircleText(id: string, text: string): Promise<void> {
  await invoke("core_circle_send", { circle: id, text });
}

/** What others said has been shown. Nothing travels: a circle tells nobody when you read. */
export async function markCircleRead(id: string): Promise<void> {
  await invoke("core_circle_mark_read", { circle: id });
}

/** Makes a circle of these contacts, of the main list or of an open session; returns its id. */
export async function createCircle(name: string, members: string[], session?: string): Promise<string> {
  const id = await invoke<string>("core_circle_create", { name: name.trim(), members, session });
  await refreshChats();
  return id;
}

export async function inviteToCircle(id: string, contact: string): Promise<void> {
  await invoke("core_circle_invite", { circle: id, contact });
  await refreshChats();
}

export async function removeFromCircle(id: string, contact: string): Promise<void> {
  await invoke("core_circle_remove", { circle: id, contact });
  await refreshChats();
}

export async function setCircleAdmin(id: string, contact: string, admin: boolean): Promise<void> {
  await invoke("core_circle_set_admin", { circle: id, contact, admin });
  await refreshChats();
}

export async function renameCircle(id: string, name: string): Promise<void> {
  await invoke("core_circle_rename", { circle: id, name: name.trim() });
  await refreshChats();
}

/** Whether only the admins write; everyone else reads. */
export async function setCircleAdminsOnly(id: string, adminsOnly: boolean): Promise<void> {
  await invoke("core_circle_admins_only", { circle: id, adminsOnly });
  await refreshChats();
}

/** Leaves the circle: every member is told; what was said stays here, read only. */
export async function leaveCircle(id: string): Promise<void> {
  await invoke("core_circle_leave", { circle: id });
  await refreshChats();
}

/** Erases a circle this phone is no longer in, with everything said in it. */
export async function forgetCircle(id: string): Promise<void> {
  await invoke("core_circle_forget", { circle: id });
  await refreshChats();
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

/** What became of a file a plugin made (A2): sent by itself, or left for the user to send. */
export interface Made {
  sent: boolean;
  staged?: PickedFile;
}

/**
 * Hands the app what a plugin made (A2). The core decides by what the user granted the plugin:
 * `auto` sends it, `propose` leaves it in the composer as a picked file, nothing refuses it.
 */
export async function pluginMade(plugin: string, contact: string, name: string, mime: string, data: string): Promise<Made> {
  return invoke<Made>("core_plugin_made", { plugin, contact, name, mime, data });
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
 * Takes this device off the router and wipes everything FlickerTalk keeps on the phone (§78),
 * and what the WebView keeps too: the app starts again empty, at the welcome, like a new install
 * (2026-09-30). There is no way back.
 */
export async function erasePhone(): Promise<void> {
  await invoke("core_erase");
  localStorage.clear();
}

/** Reconnects to the router at once (2026-09-28): the app is back on the screen. Never fails. */
export async function resumeRouter(): Promise<void> {
  await invoke("core_resume").catch(() => undefined);
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
/** How far a plugin may write in the chat (A2). */
export type Sending = "nothing" | "propose" | "auto";

export interface PluginPermissions {
  /** Hosts it may talk to; empty is no network at all. */
  network: string[];
  /** Whether it may read the message the user hands it. */
  messages: boolean;
  /** "nothing", "propose" (fills the composer) or "auto" (sends by itself). */
  send: Sending;
  /** Whether it may print what it made. */
  print?: boolean;
  /** Whether it may talk to the same plugin on the other side of a conversation (2026-09-27). */
  live?: boolean;
  /** Whether it may set reminders on this phone. */
  remind?: boolean;
  /** Whether it may keep files in the user's own cloud, through the core. */
  drive?: boolean;
  /** How much it may keep in its records: "small" (settings, notes) or "large" (boards). */
  storage?: "small" | "large";
}

export interface PluginView {
  id: string;
  name: string;
  version: string;
  asks: PluginPermissions;
  granted: PluginPermissions;
  installedAt: number;
  /** The kinds of file it opens: media types, or "*\/*" for any (2026-09-27). */
  opens?: string[];
  /** The kinds of file it is the viewer of: exact media types; a tap opens them here. */
  views?: string[];
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

const DAY = 24 * 60 * 60 * 1000;

/**
 * Whole days left until `until` (ms), as every screen says it (§41): a day that has started
 * counts, so a phone installed a moment ago has 365; nothing is ever below 0.
 */
export function daysLeft(until: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((until - now) / DAY));
}

export async function plan(): Promise<PlanView> {
  return invoke<PlanView>("core_plan");
}

/** What the user says about their age; never a date of birth (§30, §43). */
export async function setAge(age: "minor" | "adult"): Promise<void> {
  await invoke("core_set_age", { age });
}

/** Asks the Store for the subscription. No payment data ever reaches us (§47). */
/**
 * What the Store answered, as an i18n key. Backing out of the Store window is not a failure, so
 * it says nothing; anything nobody wrote is shown as a plain failure, never as it came (§84).
 */
export function payTrouble(error: unknown): string {
  const answer = String(error).trim();
  if (answer === "cancelled") return "";
  const known = ["not_on_sale", "store_unavailable", "pending_approval"];
  return `plan.trouble.${known.includes(answer) ? answer : "failed"}`;
}

/**
 * What a year costs, exactly as the Store formats it for this phone (2026-09-29), or null when it
 * cannot say (offline, a desktop, the product missing). Asked every time, never kept: a price
 * from another store or another country is never shown.
 */
export async function subscriptionPrice(): Promise<string | null> {
  try {
    const answer = await invoke<{ price: string | null } | null>("core_subscription_price");
    return answer?.price?.trim() || null;
  } catch {
    return null;
  }
}

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

// `session` (2026-10-01, §108): the hidden session the plugin is open in, if any. What a plugin
// keeps, sets or looks up belongs to that place; the core keeps each apart and the plugin never
// hears of it.

export async function pluginRead(plugin: string, key: string, session?: string): Promise<string | null> {
  return invoke<string | null>("core_plugin_read", { plugin, key, session });
}

export async function pluginWrite(plugin: string, key: string, value: string, session?: string): Promise<void> {
  await invoke("core_plugin_write", { plugin, key, value, session });
}

export async function pluginForget(plugin: string, key: string, session?: string): Promise<void> {
  await invoke("core_plugin_forget", { plugin, key, session });
}

/** What the user allows a plugin to do; never more than it asked for (§53). */
export async function grantPlugin(plugin: string, granted: PluginPermissions): Promise<void> {
  await invoke("core_plugin_grant", { plugin, granted });
}

// ---- Records, refs, reminders, the live channel and "open with" (2026-09-27) ----

/** Sent by the core on `ft://plugin`: what a plugin on the other side said to its twin here. */
export const PLUGIN_EVENT = "ft://plugin";

export interface PluginEvent {
  plugin: string;
  contact: string;
  /** Base64. */
  data: string;
}

/** A record's value, as the plugin wrote it (a string), or null. */
export async function pluginRecordGet(plugin: string, key: string, session?: string): Promise<string | null> {
  const value = await invoke<string | null>("core_plugin_record_get", { plugin, key, session });
  return value === null ? null : fromBase64(value);
}

export async function pluginRecordSet(plugin: string, key: string, value: string, session?: string): Promise<void> {
  await invoke("core_plugin_record_set", { plugin, key, value: toBase64Text(value), session });
}

export async function pluginRecordForget(plugin: string, key: string, session?: string): Promise<void> {
  await invoke("core_plugin_record_forget", { plugin, key, session });
}

export async function pluginRecordKeys(plugin: string, prefix: string, session?: string): Promise<string[]> {
  return invoke<string[]>("core_plugin_record_keys", { plugin, prefix, session });
}

/** How much of its room a plugin uses in that place and how much it has, in bytes. */
export async function pluginRecordUsage(plugin: string, session?: string): Promise<{ used: number; quota: number }> {
  const [used, quota] = await invoke<[number, number]>("core_plugin_record_usage", { plugin, session });
  return { used, quota };
}

/** An opaque handle for the message the user hands a plugin; nothing of the contact in it. */
export async function pluginRef(plugin: string, message: string): Promise<string> {
  return invoke<string>("core_plugin_ref", { plugin, message });
}

/** Where a plugin's ref leads, or null if the message or the contact is gone, or is not in that place. */
export async function pluginOpenChat(plugin: string, reference: string, session?: string): Promise<{ contact: string; message: string } | null> {
  return invoke<{ contact: string; message: string } | null>("core_plugin_open_chat", { plugin, reference, session });
}

export interface Reminder {
  plugin: string;
  id: string;
  at: number;
  text: string;
}

export async function remindSet(plugin: string, id: string, at: number, text: string, session?: string): Promise<void> {
  await invoke("core_remind_set", { plugin, id, at, text, session });
}

export async function remindCancel(plugin: string, id: string, session?: string): Promise<boolean> {
  return invoke<boolean>("core_remind_cancel", { plugin, id, session });
}

export async function remindList(plugin: string, session?: string): Promise<Reminder[]> {
  return invoke<Reminder[]>("core_remind_list", { plugin, session });
}

/**
 * The reminder the user tapped to open the app, once: `{ plugin, id }`, with the hidden session it
 * was set in if it was (2026-10-01, §108), or null.
 */
export async function pendingReminder(): Promise<{ plugin: string; id: string; session?: string } | null> {
  const tapped = await invoke<{ plugin: string; id: string; session: string | null } | null>("core_pending_reminder").catch(() => null);
  if (!tapped?.plugin) return null;
  return tapped.session ? { plugin: tapped.plugin, id: tapped.id, session: tapped.session } : { plugin: tapped.plugin, id: tapped.id };
}

/** What a plugin says to its twin on the contact's phone; false if it cannot be reached now. */
export async function pluginLiveSend(plugin: string, contact: string, data: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_live_send", { plugin, contact, data });
}

/** The file of a message, for a plugin to open it: name, kind and bytes as base64. */
export async function readMessageFile(message: string): Promise<{ name: string; mime: string; data: string }> {
  return invoke("core_read_message_file", { message });
}

/** UTF-8 text as base64, for the bytes the core keeps for a plugin. */
export function toBase64Text(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

export function fromBase64(value: string): string {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return new TextDecoder().decode(bytes);
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

// ---------------------------------------------------------------------------------------------
// The user's own cloud (plan-drive, 2026-09-27): the drive and the backup. Tokens, the vault key
// and the recovery code live in the core; here only names, sizes and states.
// ---------------------------------------------------------------------------------------------

/** Sent by the core when the drive changes, and while bytes move. */
export const VAULT_EVENT = "ft://vault";
export const VAULT_PROGRESS_EVENT = "ft://vault-progress";

export interface VaultQuota {
  used: number;
  total: number;
}

export interface DriveStatus {
  files: number;
  folders: number;
  used: number;
  pending: number;
  quota: VaultQuota | null;
  backupAt: number | null;
}

/**
 * `none`: no cloud; `empty`: logged in, no drive yet; `locked`: a drive from another phone;
 * `outdated`: a drive of the first version, made again; `ready`.
 */
export type VaultState = "none" | "empty" | "locked" | "outdated" | "ready";

export interface VaultStatus {
  state: VaultState;
  provider: string | null;
  drive: DriveStatus | null;
  problem: string | null;
  /** Wrong recovery phrases this phone may still try (2026-09-28). */
  triesLeft?: number;
  /** Until when the recovery is locked here after too many wrong phrases, if it is. */
  retryAt?: number | null;
}

export interface DriveFolder {
  id: string;
  name: string;
  parent: string | null;
  modified: number;
}

export interface DriveFile {
  id: string;
  name: string;
  parent: string | null;
  size: number;
  mime: string;
  modified: number;
}

export interface DrivePending {
  blob: string;
  name: string;
  parent: string | null;
  size: number;
  mime: string;
  error: string;
}

export interface DriveListing {
  folders: DriveFolder[];
  files: DriveFile[];
  pending: DrivePending[];
}

export interface BackupInfo {
  at: number;
  files: number;
  dbSize: number;
}

export async function vaultStatus(): Promise<VaultStatus> {
  return invoke<VaultStatus>("core_vault_status");
}

/** Logs in through the system browser; the app never sees the tokens. */
export async function vaultConnect(provider = "google"): Promise<VaultStatus> {
  return invoke<VaultStatus>("core_vault_connect", { provider });
}

/** Makes the drive, its key sealed with the phrase the user chose; the phrase is kept nowhere. */
export async function vaultSetup(phrase: string): Promise<void> {
  await invoke("core_vault_setup", { phrase });
}

/** Opens a drive from another phone with its phrase; five wrong ones lock it for a day. */
export async function vaultUnlock(phrase: string): Promise<void> {
  await invoke("core_vault_unlock", { phrase });
}

/** A strong phrase, for whoever wants the app to suggest one. */
export async function vaultSuggestPhrase(): Promise<string> {
  return invoke<string>("core_vault_suggest_phrase");
}

/** Seals the drive's key with a new phrase; the old one stops opening it. */
export async function vaultChangePhrase(phrase: string): Promise<void> {
  await invoke("core_vault_change_phrase", { phrase });
}

/** The limits of a recovery phrase, in characters, as the core counts them. */
export const PHRASE_MIN = 12;
export const PHRASE_MAX = 100;

export async function vaultDisconnect(): Promise<void> {
  await invoke("core_vault_disconnect");
}

export async function vaultList(parent: string | null): Promise<DriveListing> {
  return invoke<DriveListing>("core_vault_list", { parent });
}

export async function vaultMkdir(name: string, parent: string | null): Promise<string> {
  return invoke<string>("core_vault_mkdir", { name, parent });
}

export async function vaultRename(id: string, name: string): Promise<void> {
  await invoke("core_vault_rename", { id, name });
}

export async function vaultMove(id: string, parent: string | null): Promise<void> {
  await invoke("core_vault_move", { id, parent });
}

export async function vaultRemove(id: string): Promise<void> {
  await invoke("core_vault_remove", { id });
}

/** Puts a picked file in the drive; null when it waits for the network. */
export async function vaultUpload(file: PickedFile, parent: string | null): Promise<string | null> {
  return invoke<string | null>("core_vault_upload", { file, parent });
}

/** Keeps the file of a message in the drive, from the bubble or a plugin's `ref`. */
export async function vaultUploadMessage(message: string, parent: string | null): Promise<string | null> {
  return invoke<string | null>("core_vault_upload_message", { message, parent });
}

export async function vaultRetry(): Promise<number> {
  return invoke<number>("core_vault_retry");
}

export async function vaultCancelPending(blob: string): Promise<void> {
  await invoke("core_vault_cancel_pending", { blob });
}

/** Brings a file down, opened, ready for the composer. */
export async function vaultDownload(id: string): Promise<PickedFile> {
  return invoke<PickedFile>("core_vault_download", { id });
}

export async function vaultOpen(id: string): Promise<void> {
  await invoke("core_vault_open", { id });
}

export async function vaultSave(id: string): Promise<void> {
  await invoke("core_vault_save", { id });
}

export async function vaultSend(id: string, contact: string): Promise<string> {
  return invoke<string>("core_vault_send", { id, contact });
}

export async function vaultBackup(): Promise<BackupInfo> {
  return invoke<BackupInfo>("core_vault_backup");
}

export async function vaultBackupInfo(): Promise<BackupInfo | null> {
  return invoke<BackupInfo | null>("core_vault_backup_info");
}

/** Brings the backup down; the app restarts and swaps it in. */
export async function vaultRestore(): Promise<BackupInfo> {
  return invoke<BackupInfo>("core_vault_restore");
}

export async function pluginMayUseDrive(plugin: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_may_use_drive", { plugin }).catch(() => false);
}
