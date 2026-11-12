import { invoke } from "@tauri-apps/api/core";
import { store, type Session } from "./store";
import { toSession, type SessionView } from "./views";

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
