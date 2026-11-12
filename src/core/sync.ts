import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { hueOf } from "./format";
import { chat, circle, store, type Me } from "./store";
import { heardTyping, quietTyping, TYPING_EVENT } from "./typing";
import {
  toChat,
  toCircle,
  toCircleMessage,
  toMessage,
  toSession,
  type CircleMessageView,
  type CircleView,
  type ConversationView,
  type MessageView,
  type SessionView,
} from "./views";

export const CHANGED_EVENT = "ft://changed";
const MESSAGE_LIMIT = 200;

const loaded = new Set<string>();
const loadedCircles = new Set<string>();

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

export async function loadCircleMessages(id: string): Promise<void> {
  loadedCircles.add(id);
  const views = await invoke<CircleMessageView[]>("core_circle_messages", { circle: id, limit: MESSAGE_LIMIT });
  const target = circle(id);
  if (target) target.messages = views.map(toCircleMessage);
}
