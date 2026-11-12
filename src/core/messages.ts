import { invoke } from "@tauri-apps/api/core";
import type { ChatMessage } from "./store";
import { toMessage, type MessageView } from "./views";

export async function resend(message: string): Promise<void> {
  await invoke("core_resend", { message });
}

export async function sendText(contact: string, text: string, replyTo?: string): Promise<string> {
  return invoke<string>("core_send", replyTo ? { contact, text, replyTo } : { contact, text });
}

export async function markRead(contact: string): Promise<void> {
  await invoke("core_mark_read", { contact });
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
