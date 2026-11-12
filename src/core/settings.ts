import { invoke } from "@tauri-apps/api/core";
import { store } from "./store";

export type Day = "all" | "none" | { from: string; to: string };

// Monday first
export interface Week {
  days: Day[];
}

export async function enablePush(): Promise<void> {
  await invoke("core_enable_push").catch(() => undefined);
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

export async function setName(name: string): Promise<void> {
  const trimmed = name.trim();
  await invoke("core_set_name", { name: trimmed });
  store.me.name = trimmed;
}

export async function setMailbox(enabled: boolean): Promise<void> {
  await invoke("core_set_mailbox", { enabled });
  store.me.mailbox = enabled;
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
