import { convertFileSrc } from "@tauri-apps/api/core";
import { clock, formatSize, hueOf } from "./format";
import {
  chat,
  circle,
  type Chat,
  type ChatFile,
  type ChatMessage,
  type Circle,
  type CircleKind,
  type CircleMember,
  type CircleMessage,
  type FileState,
  type MessageNote,
  type Quote,
  type Reactions,
  type Session,
  type Status,
} from "./store";

interface FileView {
  name: string;
  size: number;
  mime: string;
  progress: number;
  state: "transferring" | "waiting" | "done" | "failed";
  path: string;
}

export interface MessageView {
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

export interface ConversationView {
  id: string;
  name: string;
  unread: number;
  blocked: boolean;
  connected: boolean;
  last: MessageView | null;
}

export interface CircleMessageView {
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

export interface CircleView {
  id: string;
  name: string;
  members: CircleMember[];
  admin: boolean;
  adminsOnly: boolean;
  left: boolean;
  unread: number;
  last: CircleMessageView | null;
}

export interface SessionView {
  id: string;
  conversations: ConversationView[];
  requests: ConversationView[];
  circles?: CircleView[];
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

export function toMessage(view: MessageView, connected = false): ChatMessage {
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

export function toChat(view: ConversationView): Chat {
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

export function toCircleMessage(view: CircleMessageView): CircleMessage {
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

export function toCircle(view: CircleView): Circle {
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

export function toSession(view: SessionView): Session {
  return {
    id: view.id,
    chats: view.conversations.map(toChat),
    requests: view.requests.map(toChat),
    circles: (view.circles ?? []).map(toCircle),
  };
}
