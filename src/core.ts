// keys, the route capability and the network never reach the WebView
import { reactive } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { i18n } from "./i18n";

export type Status = "unsent" | "pending" | "sent" | "delivered" | "read";

export type FileState = "sending" | "receiving" | "paused" | "waiting" | "done" | "failed";

export interface ChatFile {
  name: string;
  size: string;
  bytes?: number;
  mime: string;
  progress: number;
  state: FileState;
  url?: string;
}

export interface ChatMessage {
  id: string;
  mine: boolean;
  text: string;
  time: string;
  // ms, 0 = unknown
  sentAt: number;
  status?: Status;
  kind?: "file";
  file?: ChatFile;
  quote?: Quote;
  reactions?: Reactions;
  pinned?: boolean;
  edited?: boolean;
  deleted?: boolean;
  scheduledFor?: number;
  updatePending?: boolean;
  notes?: MessageNote[];
}

export interface MessageNote {
  plugin: string;
  text: string;
}

export interface Reactions {
  mine?: string;
  theirs?: string;
}

export interface Quote {
  id: string;
  text: string;
  mine: boolean;
  kind: "text" | "file" | "gone";
}

export interface Chat {
  id: string;
  name: string;
  hue: number;
  connected: boolean;
  unread: number;
  time: string;
  preview: string;
  lastMine: boolean;
  lastKind: "text" | "file" | "voice";
  status: Status | "";
  blocked: boolean;
  messages: ChatMessage[];
}

export interface CircleMember {
  id: string;
  name: string;
  admin: boolean;
  me: boolean;
}

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
  notes?: MessageNote[];
}

// no server knows a circle exists, each message goes to each member directly
export interface Circle {
  id: string;
  name: string;
  hue: number;
  members: CircleMember[];
  admin: boolean;
  adminsOnly: boolean;
  left: boolean;
  unread: number;
  time: string;
  preview: string;
  lastMine: boolean;
  lastSender: string;
  status: Status | "";
  messages: CircleMessage[];
}

export interface Me {
  id: string;
  name: string;
  hue: number;
  mailbox: boolean;
  receipts: boolean;
  freeUntil: number;
  autoDownload: number;
}

export interface ContactRules {
  muted: boolean;
  acceptsChat: boolean;
  acceptsCalls: boolean;
  receipts: boolean;
  typing: boolean;
}

export type Day = "all" | "none" | { from: string; to: string };

// Monday first
export interface Week {
  days: Day[];
}

export interface ContactDetails {
  id: string;
  name: string;
  fingerprint: string;
  mailbox: boolean;
  blocked: boolean;
  // seconds, 0 = forever
  keepFor: number;
  // seconds, 0 = never
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
  quote?: Quote;
  reactions?: Reactions;
  pinned?: boolean;
  edited?: boolean;
  deleted?: boolean;
  scheduledFor?: number;
  updatePending?: boolean;
  notes?: MessageNote[];
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
  notes?: MessageNote[];
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

// opened with a PIN only, no name, and only its creator knows it exists
export interface Session {
  // "" for a session that is only on the screen
  id: string;
  chats: Chat[];
  requests: Chat[];
  circles: Circle[];
  pin?: string;
}

interface SessionView {
  id: string;
  conversations: ConversationView[];
  requests: ConversationView[];
  circles?: CircleView[];
}

export const CHANGED_EVENT = "ft://changed";
export const TYPING_EVENT = "ft://typing";
// ms
export const TYPING_FADE = 6000;
// ms
export const TYPING_EVERY = 3000;
const MESSAGE_LIMIT = 200;
const UPLOAD_SLICE = 512 * 1024;

export const store = reactive({
  ready: false,
  me: { id: "", name: "", hue: 0, mailbox: true, receipts: true, freeUntil: 0, autoDownload: 0 } as Me,
  chats: [] as Chat[],
  requests: [] as Chat[],
  // an empty list must look exactly like having none
  sessions: [] as Session[],
  circles: [] as Circle[],
  typing: {} as Record<string, boolean>,
  supportId: "",
});

const typingFades = new Map<string, ReturnType<typeof setTimeout>>();

export function heardTyping(contact: string): void {
  store.typing[contact] = true;
  clearTimeout(typingFades.get(contact));
  typingFades.set(contact, setTimeout(() => quietTyping(contact), TYPING_FADE));
}

function quietTyping(contact: string): void {
  clearTimeout(typingFades.get(contact));
  typingFades.delete(contact);
  delete store.typing[contact];
}

export function isTyping(contact: string): boolean {
  return store.typing[contact] === true;
}

const typingSent = new Map<string, number>();

// the core sends it only over an open direct connection
export async function sendTyping(contact: string): Promise<void> {
  const now = Date.now();
  if (now - (typingSent.get(contact) ?? 0) < TYPING_EVERY) return;
  typingSent.set(contact, now);
  await invoke("core_typing", { contact }).catch(() => undefined);
}

const loaded = new Set<string>();
const loadedCircles = new Set<string>();

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

export function formatSize(bytes: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  const digits = unit === 0 || value >= 10 ? 0 : 1;
  const shown = new Intl.NumberFormat(i18n.global.locale.value, { maximumFractionDigits: digits }).format(value);
  return `${shown} ${units[unit]}`;
}

function toFile(view: FileView, mine: boolean, connected: boolean): ChatFile {
  let state: FileState;
  if (view.state === "done" || view.state === "failed" || view.state === "waiting") state = view.state;
  else if (!connected) state = "paused";
  else state = mine ? "sending" : "receiving";
  const playable = ["image/", "audio/", "video/"].some((kind) => view.mime.startsWith(kind));
  const showable = playable && (mine || view.state === "done");
  return {
    name: view.name,
    size: formatSize(view.size),
    bytes: view.size,
    mime: view.mime,
    progress: view.progress,
    state,
    url: showable ? convertFileSrc(view.path) : undefined,
  };
}

function toMessage(view: MessageView, connected = false): ChatMessage {
  const message: ChatMessage = { id: view.id, mine: view.outgoing, text: view.text, time: clock(view.sentAt), sentAt: view.sentAt, status: view.state };
  if (view.file) {
    message.kind = "file";
    message.file = toFile(view.file, view.outgoing, connected);
  }
  if (view.quote) message.quote = view.quote;
  if (view.reactions) message.reactions = view.reactions;
  if (view.pinned) message.pinned = true;
  if (view.edited) message.edited = true;
  if (view.deleted) message.deleted = true;
  if (view.scheduledFor) message.scheduledFor = view.scheduledFor;
  if (view.updatePending) message.updatePending = true;
  if (view.notes?.length) message.notes = view.notes;
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

export function requestPending(id: string): boolean {
  return store.requests.some((one) => one.id === id) || store.sessions.some((session) => session.requests.some((one) => one.id === id));
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
    ...(view.notes?.length ? { notes: view.notes } : {}),
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

export function circleHome(id: string): { session?: string; contacts: Chat[] } {
  for (const session of store.sessions) {
    if (session.circles.some((candidate) => candidate.id === id)) return { session: session.id, contacts: session.chats };
  }
  return { contacts: store.chats };
}

export function sessionOf(contact: string): string | undefined {
  if (!contact) return undefined;
  return store.sessions.find((session) => session.chats.concat(session.requests).some((one) => one.id === contact))?.id || undefined;
}

export function circle(id: string): Circle | undefined {
  return store.circles.concat(...store.sessions.map((session) => session.circles)).find((candidate) => candidate.id === id);
}

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

// every PIN is valid and nothing tells whether a session existed
export async function openSession(pin: string): Promise<void> {
  const view = await invoke<SessionView | null>("core_session_open", { pin });
  showSession(view ? toSession(view) : { id: "", pin, chats: [], requests: [], circles: [] });
}

export async function closeSession(session: string): Promise<void> {
  if (session) await invoke("core_session_close", { session });
  store.sessions = store.sessions.filter((candidate) => candidate.id !== session);
}

export async function removeSession(session: string): Promise<void> {
  await invoke("core_session_remove", { session });
  store.sessions = store.sessions.filter((candidate) => candidate.id !== session);
}

export async function acceptContact(contact: string): Promise<void> {
  await invoke("core_accept_contact", { contact });
  await refreshChats();
}

export async function declineContact(contact: string): Promise<void> {
  await invoke("core_decline_contact", { contact });
  await refreshChats();
}

export async function addSupportContact(): Promise<void> {
  try {
    await invoke("core_add_support_contact");
    await refreshChats();
  } catch {
    // no support contact is fine
  }
}

export function isSupport(contact: string): boolean {
  return store.supportId !== "" && contact === store.supportId;
}

export async function removeContact(contact: string): Promise<void> {
  await invoke("core_remove_contact", { contact });
  await refreshChats();
}

export async function renewLink(session?: string): Promise<string> {
  return invoke<string>("core_renew_link", session ? { session } : {});
}

export async function resend(message: string): Promise<void> {
  await invoke("core_resend", { message });
}

export async function acceptFile(message: string): Promise<void> {
  await invoke("core_accept_file", { message });
}

export const AUTO_DOWNLOAD_CHOICES = [0, 10_000_000, 100_000_000, 1_000_000_000, Number.MAX_SAFE_INTEGER] as const;

// limits saved before the choices were decimal map to the nearest one
export function autoDownloadChoice(bytes: number): number {
  if (AUTO_DOWNLOAD_CHOICES.includes(bytes as (typeof AUTO_DOWNLOAD_CHOICES)[number])) return bytes;
  const sizes = AUTO_DOWNLOAD_CHOICES.slice(1, -1);
  const distance = (size: number) => Math.abs(Math.log(bytes / size));
  return sizes.reduce((best, size) => (distance(size) < distance(best) ? size : best));
}

// 0 = always ask
export async function setAutoDownload(bytes: number): Promise<void> {
  await invoke("core_set_auto_download", { bytes });
  store.me.autoDownload = bytes;
}

export async function start(): Promise<void> {
  const me = await invoke<Omit<Me, "hue">>("core_me");
  store.me = { ...me, hue: hueOf(me.id) };
  store.supportId = (await invoke<string | null>("core_support_id").catch(() => null)) ?? "";
  await refreshChats();
  await listen<{ contact: string }>(TYPING_EVENT, ({ payload }) => heardTyping(payload.contact));
  await listen<{ contact: string | null; circle?: string | null; all?: boolean }>(CHANGED_EVENT, ({ payload }) => {
    if (payload.contact) quietTyping(payload.contact);
    // the list first, an open chat's files depend on the connection it reports
    void refreshChats().then(() => {
      // events were lost to a burst, refresh everything
      if (payload.all) {
        return Promise.all([...[...loaded].map(loadMessages), ...[...loadedCircles].map(loadCircleMessages)]).then(() => {});
      }
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

export async function sendText(contact: string, text: string, replyTo?: string): Promise<string> {
  return invoke<string>("core_send", replyTo ? { contact, text, replyTo } : { contact, text });
}

export async function loadCircleMessages(id: string): Promise<void> {
  loadedCircles.add(id);
  const views = await invoke<CircleMessageView[]>("core_circle_messages", { circle: id, limit: MESSAGE_LIMIT });
  const target = circle(id);
  if (target) target.messages = views.map(toCircleMessage);
}

export async function sendCircleText(id: string, text: string): Promise<string> {
  return invoke<string>("core_circle_send", { circle: id, text });
}

// nothing travels, a circle tells nobody when you read
export async function markCircleRead(id: string): Promise<void> {
  await invoke("core_circle_mark_read", { circle: id });
}

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

export async function setCircleAdminsOnly(id: string, adminsOnly: boolean): Promise<void> {
  await invoke("core_circle_admins_only", { circle: id, adminsOnly });
  await refreshChats();
}

export async function leaveCircle(id: string): Promise<void> {
  await invoke("core_circle_leave", { circle: id });
  await refreshChats();
}

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

export interface PickedFile {
  path: string;
  name: string;
  mime: string;
  size: number;
}

// on Android the WebView's file input opens a screen you can't leave without picking something
export async function pickFiles(accept = ""): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_pick_files", { accept });
}

export async function takePhoto(): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_take_photo");
}

// no path reaches the WebView
export async function pickForPlugin(accept = ""): Promise<{ name: string; mime: string; data: string } | null> {
  return invoke("core_pick_for_plugin", { accept });
}

export async function takePhotoForPlugin(): Promise<{ name: string; mime: string; data: string } | null> {
  return invoke("core_take_photo_for_plugin");
}

export interface Made {
  sent: boolean;
  staged?: PickedFile;
}

export async function pluginMade(plugin: string, contact: string, name: string, mime: string, data: string): Promise<Made> {
  return invoke<Made>("core_plugin_made", { plugin, contact, name, mime, data });
}

// the bytes never go through the WebView
export async function sendPicked(contact: string, file: PickedFile): Promise<void> {
  await invoke("core_send_picked", { contact, file });
}

// base64 slices, the Android IPC only carries JSON
export async function sendFile(contact: string, file: File): Promise<void> {
  const upload = await invoke<string>("core_upload_start");
  for (let offset = 0; offset < file.size; offset += UPLOAD_SLICE) {
    const data = await toBase64(file.slice(offset, offset + UPLOAD_SLICE));
    await invoke("core_upload_append", { upload, data });
  }
  await invoke("core_send_file", { contact, upload, name: file.name, mime: file.type || "application/octet-stream" });
}

export async function enablePush(): Promise<void> {
  await invoke("core_enable_push").catch(() => undefined);
}

export async function openFile(message: string): Promise<void> {
  await invoke("core_open_file", { message });
}

export async function saveFile(message: string): Promise<void> {
  await invoke("core_save_file", { message });
}

export async function markRead(contact: string): Promise<void> {
  await invoke("core_mark_read", { contact });
}

export async function myCardLink(session?: string): Promise<string> {
  return invoke<string>("core_card", session ? { session } : undefined);
}

export async function erasePhone(): Promise<void> {
  await invoke("core_erase");
  localStorage.clear();
}

export async function resumeRouter(): Promise<void> {
  await invoke("core_resume").catch(() => undefined);
}

export async function shareText(text: string): Promise<void> {
  await invoke("core_share", { text });
}

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

export async function setReceipts(enabled: boolean): Promise<void> {
  await invoke("core_set_receipts", { enabled });
  store.me.receipts = enabled;
}

export async function quietHours(): Promise<Week | null> {
  const json = await invoke<string | null>("core_quiet_hours");
  return json ? (JSON.parse(json) as Week) : null;
}

export async function setQuietHours(week: Week | null): Promise<void> {
  await invoke("core_set_quiet_hours", { hours: week ? JSON.stringify(week) : null });
}

const REPORT_ADDRESS = "info@flickertalk.com";

// messages go as evidence only if the user chose to
export function reportLink(contact: string, reason: string, evidence: string[]): string {
  const lines = [`Reported: ${contact}`, `Reason: ${reason}`];
  if (evidence.length) lines.push("", "Evidence:", ...evidence.map((text) => `> ${text}`));
  const subject = encodeURIComponent(`Report ${contact}`);
  return `mailto:${REPORT_ADDRESS}?subject=${subject}&body=${encodeURIComponent(lines.join("\n"))}`;
}

export type Sending = "nothing" | "propose" | "auto";

export interface PluginPermissions {
  // empty means no network
  network: string[];
  messages: boolean;
  send: Sending;
  print?: boolean;
  live?: boolean;
  remind?: boolean;
  drive?: boolean;
  storage?: "small" | "large";
  location?: boolean;
  translate?: boolean;
}

export interface PluginView {
  id: string;
  name: string;
  version: string;
  asks: PluginPermissions;
  granted: PluginPermissions;
  installedAt: number;
  /** "*\/*" = any */
  opens?: string[];
  views?: string[];
  kind?: PluginKind;
  locales?: PluginLocales;
  icon?: string;
  image?: string;
  hooks?: PluginHooks;
}

export type PluginHookName = "message" | "composer" | "settings";

export interface PluginHookDeclaration {
  label: string;
  icon: string;
  fields?: PluginSettingsField[];
  note?: string;
}

export interface PluginSettingsField {
  key: string;
  type: "select" | "toggle";
  label: string;
}

export type PluginHooks = Partial<Record<PluginHookName, PluginHookDeclaration>>;

export interface PluginSettingsLabels {
  label?: string;
  fields?: Partial<Record<string, string>>;
  note?: string;
}

export type PluginLocales = Record<
  string,
  { name?: string; summary?: string; hooks?: Partial<Record<string, string | PluginSettingsLabels>> }
>;

export type PluginKind = "tool" | "game";

export async function plugins(): Promise<PluginView[]> {
  return invoke<PluginView[]>("core_plugins");
}

export interface OfferedPlugin {
  id: string;
  name: string;
  version: string;
  summary: string;
  size: number;
  installed: boolean;
  carried: boolean;
  kind?: PluginKind;
  locales?: PluginLocales;
  icon?: string;
  image?: string;
}

// the core never updates a plugin while a frame is open
export async function pluginOpen(plugin: string, open: boolean): Promise<void> {
  await invoke("core_plugin_open", { plugin, open });
}

export async function offeredPlugins(): Promise<OfferedPlugin[]> {
  return invoke<OfferedPlugin[]>("core_catalogue");
}

export async function installPlugin(plugin: string): Promise<void> {
  await invoke("core_plugin_add", { plugin });
}

export interface PlanView {
  state: "trial" | "subscribed" | "limited";
  // ms, 0 = neither
  until: number;
  // Play keeps renewing, so until is only how long the last check holds. StoreKit's is a real expiry.
  renews?: boolean;
}

const DAY = 24 * 60 * 60 * 1000;

export function daysLeft(until: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((until - now) / DAY));
}

export async function plan(): Promise<PlanView> {
  return invoke<PlanView>("core_plan");
}

export const PREMIUM_PAGE = "/tabs/settings#premium";

export const PLAN_EVENT = "ft://plan";

export async function followPlan(changed: () => void): Promise<() => void> {
  return listen(PLAN_EVENT, () => changed()).catch(() => () => undefined);
}

export const NEEDS_SUBSCRIPTION = "needs_subscription";

export function needsSubscription(error: unknown): boolean {
  return String(error instanceof Error ? error.message : error) === NEEDS_SUBSCRIPTION;
}

// unknown Store errors show as a plain failure, never as they came
export function payTrouble(error: unknown): string {
  const answer = String(error).trim();
  if (answer === "cancelled") return "";
  const known = ["not_on_sale", "store_unavailable", "pending_approval"];
  return `plan.trouble.${known.includes(answer) ? answer : "failed"}`;
}

// never cached, a price from another store or country must not show
export async function subscriptionPrice(): Promise<string | null> {
  try {
    const answer = await invoke<{ price: string | null } | null>("core_subscription_price");
    return answer?.price?.trim() || null;
  } catch {
    return null;
  }
}

export type FeedbackOutcome = "sent" | "tooMany" | "failed";

const FEEDBACK_OUTCOMES: readonly FeedbackOutcome[] = ["sent", "tooMany", "failed"];

export async function sendFeedback(text: string): Promise<FeedbackOutcome> {
  try {
    const outcome = await invoke<FeedbackOutcome>("core_send_feedback", { text });
    return FEEDBACK_OUTCOMES.includes(outcome) ? outcome : "failed";
  } catch {
    return "failed";
  }
}

export async function subscribe(): Promise<void> {
  await invoke("core_subscribe");
}

export type RestoreOutcome = "restored" | "nothing";

export async function restoreSubscription(): Promise<RestoreOutcome> {
  return (await invoke<string>("core_restore_subscription")) === "restored" ? "restored" : "nothing";
}

export async function scheduleText(contact: string, text: string, sendAt: number, replyTo?: string): Promise<string> {
  return invoke<string>("core_schedule", replyTo ? { contact, text, sendAt, replyTo } : { contact, text, sendAt });
}

// on this phone only
export async function searchMessages(contact: string, query: string): Promise<ChatMessage[]> {
  if (!query.trim()) return [];
  const views = await invoke<MessageView[]>("core_search", { contact, query: query.trim() });
  return views.map((view) => toMessage(view));
}

export async function editMessage(message: string, text: string): Promise<void> {
  await invoke("core_edit", { message, text: text.trim() });
}

export async function deleteForEveryone(message: string): Promise<void> {
  await invoke("core_delete_everyone", { message });
}

// local only, nothing travels
export async function pinMessage(message: string, pinned: boolean): Promise<void> {
  await invoke("core_pin", { message, pinned });
}

export async function setMessageNote(message: string, plugin: string, text: string): Promise<void> {
  await invoke("core_message_note_set", { message, plugin, text });
}

export async function clearMessageNote(message: string, plugin: string): Promise<void> {
  await invoke("core_message_note_clear", { message, plugin });
}

export interface ChatHookState {
  plugin: string;
  autoMessage: boolean;
  autoComposer: boolean;
  lang?: string;
}

export async function chatHooks(conversation: string): Promise<ChatHookState[]> {
  try {
    return (await invoke<ChatHookState[] | undefined>("core_chat_hooks", { conversation })) ?? [];
  } catch {
    return [];
  }
}

export async function setChatHooks(
  conversation: string,
  plugin: string,
  said: { message: boolean; composer: boolean; lang?: string | null },
): Promise<void> {
  await invoke("core_chat_hooks_set", { conversation, plugin, autoMessage: said.message, autoComposer: said.composer, lang: said.lang || null });
}

export async function hookSample(plugin: string, conversation: string): Promise<string> {
  try {
    return (await invoke<string | undefined>("core_hook_sample", { plugin, conversation })) ?? "";
  } catch {
    return "";
  }
}

export async function react(contact: string, message: string, emoji: string | null): Promise<void> {
  await invoke("core_react", emoji ? { contact, message, emoji } : { contact, message });
}

export async function forgetMessage(message: string): Promise<void> {
  await invoke("core_forget_message", { message });
}

export async function forwardMessage(message: string, contact: string): Promise<void> {
  await invoke("core_forward", { message, contact });
}

export async function shareMessage(message: string): Promise<void> {
  await invoke("core_share_message", { message });
}

export async function pluginSave(name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_save", { name, mime, data });
}

export async function pluginPrint(plugin: string, name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_print", { plugin, name, mime, data });
}

// metres, ms
export interface PluginLocation {
  lat: number;
  lon: number;
  accuracy: number;
  at: number;
}

export async function pluginLocation(plugin: string): Promise<PluginLocation | null> {
  return invoke<PluginLocation | null>("core_plugin_location", { plugin });
}

// the core checks the grant, the text and the tags first, and the text never leaves the phone

export type TranslationStatus = "installed" | "downloadable" | "unsupported";

export async function translateLanguages(plugin: string): Promise<string[]> {
  return invoke<string[]>("core_translate_languages", { plugin });
}

export async function translateDetect(plugin: string, text: string): Promise<string | null> {
  return invoke<string | null>("core_translate_detect", { plugin, text });
}

export async function translateStatus(plugin: string, from: string, to: string): Promise<TranslationStatus> {
  return invoke<TranslationStatus>("core_translate_status", { plugin, from, to });
}

// iOS may show its own sheet
export async function translatePrepare(plugin: string, from: string, to: string): Promise<boolean> {
  return invoke<boolean>("core_translate_prepare", { plugin, from, to });
}

// Android only
export async function translateRemove(plugin: string, from: string, to: string): Promise<boolean> {
  return invoke<boolean>("core_translate_remove", { plugin, from, to });
}

export async function translateText(plugin: string, text: string, from: string, to: string): Promise<string | null> {
  return invoke<string | null>("core_translate_text", { plugin, text, from, to });
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

// stored per place (session), the plugin never hears of sessions

export async function pluginRead(plugin: string, key: string, session?: string): Promise<string | null> {
  return invoke<string | null>("core_plugin_read", { plugin, key, session });
}

export async function pluginWrite(plugin: string, key: string, value: string, session?: string): Promise<void> {
  await invoke("core_plugin_write", { plugin, key, value, session });
}

export async function pluginForget(plugin: string, key: string, session?: string): Promise<void> {
  await invoke("core_plugin_forget", { plugin, key, session });
}

export async function grantPlugin(plugin: string, granted: PluginPermissions): Promise<void> {
  await invoke("core_plugin_grant", { plugin, granted });
}

export const PLUGIN_EVENT = "ft://plugin";

export interface PluginEvent {
  plugin: string;
  contact: string;
  // base64
  data: string;
}

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

// bytes
export async function pluginRecordUsage(plugin: string, session?: string): Promise<{ used: number; quota: number }> {
  const [used, quota] = await invoke<[number, number]>("core_plugin_record_usage", { plugin, session });
  return { used, quota };
}

// opaque, nothing of the contact in it
export async function pluginRef(plugin: string, message: string): Promise<string> {
  return invoke<string>("core_plugin_ref", { plugin, message });
}

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

export async function pendingReminder(): Promise<{ plugin: string; id: string; session?: string } | null> {
  const tapped = await invoke<{ plugin: string; id: string; session: string | null } | null>("core_pending_reminder").catch(() => null);
  if (!tapped?.plugin) return null;
  return tapped.session ? { plugin: tapped.plugin, id: tapped.id, session: tapped.session } : { plugin: tapped.plugin, id: tapped.id };
}

// stable on this phone, different for each plugin
export async function pluginChat(plugin: string, contact: string): Promise<string> {
  return invoke<string>("core_plugin_chat", { plugin, contact });
}

export async function pluginLiveSend(plugin: string, contact: string, data: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_live_send", { plugin, contact, data });
}

export async function readMessageFile(message: string): Promise<{ name: string; mime: string; data: string }> {
  return invoke("core_read_message_file", { message });
}

export async function fileBelongsTo(file: string, contact: string): Promise<boolean> {
  return invoke<boolean>("core_file_belongs_to", { file, contact }).catch(() => false);
}

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

// the name never leaves the phone
export async function renameContact(contact: string, name: string): Promise<void> {
  await invoke("core_rename", { contact, name: name.trim() });
  const chat = store.chats.find((item) => item.id === contact);
  if (chat) chat.name = name.trim();
}

// seconds, 0 = forever / never
export async function setHistory(contact: string, keepFor: number, burnAfterRead: number): Promise<void> {
  await invoke("core_set_history", { contact, keepFor, burnAfterRead });
}

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

// tokens, the vault key and the recovery code stay in the core

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

export type VaultState = "none" | "empty" | "locked" | "outdated" | "ready";

export interface VaultStatus {
  state: VaultState;
  provider: string | null;
  drive: DriveStatus | null;
  problem: string | null;
  triesLeft?: number;
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

// the app never sees the tokens
export async function vaultConnect(provider = "google"): Promise<VaultStatus> {
  return invoke<VaultStatus>("core_vault_connect", { provider });
}

// the phrase is kept nowhere
export async function vaultSetup(phrase: string): Promise<void> {
  await invoke("core_vault_setup", { phrase });
}

export async function vaultUnlock(phrase: string): Promise<void> {
  await invoke("core_vault_unlock", { phrase });
}

export async function vaultSuggestPhrase(): Promise<string> {
  return invoke<string>("core_vault_suggest_phrase");
}

export async function vaultChangePhrase(phrase: string): Promise<void> {
  await invoke("core_vault_change_phrase", { phrase });
}

// counted as the core counts them
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

export async function vaultUploadPicked(parent: string | null): Promise<number> {
  return invoke<number>("core_vault_upload_picked", { parent });
}

export async function vaultUploadMessage(message: string, parent: string | null): Promise<string | null> {
  return invoke<string | null>("core_vault_upload_message", { message, parent });
}

export async function vaultRetry(): Promise<number> {
  return invoke<number>("core_vault_retry");
}

export async function vaultCancelPending(blob: string): Promise<void> {
  await invoke("core_vault_cancel_pending", { blob });
}

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

export async function vaultRestore(): Promise<BackupInfo> {
  return invoke<BackupInfo>("core_vault_restore");
}

export async function pluginMayUseDrive(plugin: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_may_use_drive", { plugin }).catch(() => false);
}
