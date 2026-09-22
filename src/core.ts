/**
 * Bridge to the Rust core (Plan §82, §106 M3): the UI shows what the core has and forwards the
 * user's intents. Keys, the route capability and the network never reach the WebView (§54).
 */
import { reactive } from "vue";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  status: Status | "";
  blocked: boolean;
  messages: ChatMessage[];
}

export interface Me {
  id: string;
  name: string;
  hue: number;
  mailbox: boolean;
}

export interface ContactDetails {
  id: string;
  name: string;
  fingerprint: string;
  mailbox: boolean;
  blocked: boolean;
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

export const CHANGED_EVENT = "ft://changed";
const MESSAGE_LIMIT = 200;
/** Bytes per call when copying a picked file into the app. */
const UPLOAD_SLICE = 512 * 1024;

export const store = reactive({
  ready: false,
  me: { id: "", name: "", hue: 0, mailbox: true } as Me,
  chats: [] as Chat[],
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
  const showable = view.mime.startsWith("image/") && (mine || view.state === "done");
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
    status: view.last?.state ?? "",
    blocked: view.blocked,
    messages: chat(view.id)?.messages ?? [],
  };
}

export function chat(id: string): Chat | undefined {
  return store.chats.find((candidate) => candidate.id === id);
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

/** This device's Contact Card as a link: shown as a QR code and shared. */
export async function myCardLink(): Promise<string> {
  return invoke<string>("core_card");
}

/** Adds the owner of a scanned or pasted card and returns their id. */
export async function addContact(link: string): Promise<string> {
  return invoke<string>("core_add_contact", { link: link.trim() });
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

export async function contactDetails(contact: string): Promise<ContactDetails> {
  return invoke<ContactDetails>("core_contact", { contact });
}

export async function block(contact: string, blocked: boolean): Promise<void> {
  await invoke("core_block", { contact, blocked });
  await refreshChats();
}
