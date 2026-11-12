import { invoke } from "@tauri-apps/api/core";

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
