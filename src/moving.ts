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

export async function invite(): Promise<string> {
  const link = await invoke<string>("core_move_invite");
  move.phase = "waiting";
  return link;
}

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
      setOnboarded();
      move.phase = "received";
      break;
    case "sent":
      clearOnboarded();
      move.phase = "sent";
      break;
    case "failed":
      move.phase = "failed";
      break;
  }
}

export async function startMoving(): Promise<void> {
  if (listening) return;
  listening = true;
  await listen<MoveEvent>(MOVE_EVENT, ({ payload }) => onEvent(payload));
}
