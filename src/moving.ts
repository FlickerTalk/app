/**
 * Moving to a new phone (Plan §60, §106 M7). The new phone shows an invite as a QR code; the old
 * phone scans it and the core hands over the identity, contacts and history, directly. The new
 * phone then starts again with them; the old one is erased and starts again empty.
 */
import { reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { clearOnboarded, setOnboarded } from "./preferences";

export type MovePhase = "idle" | "waiting" | "moving" | "received" | "sent" | "failed";

interface MoveEvent {
  kind: "progress" | "received" | "sent" | "failed";
  done?: number;
  total?: number;
}

export const MOVE_EVENT = "ft://move";

const idle = () => ({ phase: "idle" as MovePhase, done: 0, total: 0, error: "" });
export const move = reactive(idle());

let listening = false;

export function reset() {
  Object.assign(move, idle());
}

/** New phone: the invite to show as a QR code. */
export async function invite(): Promise<string> {
  const link = await invoke<string>("core_move_invite");
  move.phase = "waiting";
  return link;
}

/** Old phone: hands everything to the phone whose invite was scanned or pasted. */
export async function moveTo(link: string): Promise<void> {
  move.phase = "moving";
  try {
    await invoke("core_move_to", { link: link.trim() });
  } catch (error) {
    Object.assign(move, { phase: "failed", error: String(error) });
  }
}

function onEvent(event: MoveEvent) {
  switch (event.kind) {
    case "progress":
      Object.assign(move, { phase: "moving", done: event.done ?? 0, total: event.total ?? 0 });
      break;
    case "received":
      // The app starts again by itself with the moved identity: straight to the chats.
      setOnboarded();
      move.phase = "received";
      break;
    case "sent":
      // This phone is erased: it starts again as a new install.
      clearOnboarded();
      move.phase = "sent";
      break;
    case "failed":
      move.phase = "failed";
      break;
  }
}

/** Listens to the core's move events; once, at start. */
export async function startMoving(): Promise<void> {
  if (listening) return;
  listening = true;
  await listen<MoveEvent>(MOVE_EVENT, ({ payload }) => onEvent(payload));
}
