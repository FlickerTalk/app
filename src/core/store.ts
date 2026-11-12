import { reactive } from "vue";

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

// opened with a PIN only, no name, and only its creator knows it exists
export interface Session {
  // "" for a session that is only on the screen
  id: string;
  chats: Chat[];
  requests: Chat[];
  circles: Circle[];
  pin?: string;
}

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

export function chat(id: string): Chat | undefined {
  return allChats().find((candidate) => candidate.id === id);
}

export function requestPending(id: string): boolean {
  return store.requests.some((one) => one.id === id) || store.sessions.some((session) => session.requests.some((one) => one.id === id));
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
