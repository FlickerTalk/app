import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { store } from "./store";
import { refreshChats } from "./sync";

export interface ContactRules {
  muted: boolean;
  acceptsChat: boolean;
  acceptsCalls: boolean;
  receipts: boolean;
  typing: boolean;
}

export interface ContactDetails {
  id: string;
  name: string;
  fingerprint: string;
  mailbox: boolean;
  blocked: boolean;
  // seconds, 0 = forever
  keepFor: number;
  // seconds, 0 = never
  burnAfterRead: number;
  rules: ContactRules;
}

export async function acceptContact(contact: string): Promise<void> {
  await invoke("core_accept_contact", { contact });
  await refreshChats();
}

export async function declineContact(contact: string): Promise<void> {
  await invoke("core_decline_contact", { contact });
  await refreshChats();
}

export async function addSupportContact(): Promise<void> {
  try {
    await invoke("core_add_support_contact");
    await refreshChats();
  } catch {
    // no support contact is fine
  }
}

export function isSupport(contact: string): boolean {
  return store.supportId !== "" && contact === store.supportId;
}

export async function removeContact(contact: string): Promise<void> {
  await invoke("core_remove_contact", { contact });
  await refreshChats();
}

export async function renewLink(session?: string): Promise<string> {
  return invoke<string>("core_renew_link", session ? { session } : {});
}

export async function myCardLink(session?: string): Promise<string> {
  return invoke<string>("core_card", session ? { session } : undefined);
}

export async function addContact(link: string, session?: string): Promise<string> {
  return invoke<string>("core_add_contact", { link: link.trim(), session });
}

export async function setRules(contact: string, rules: ContactRules): Promise<void> {
  await invoke("core_set_rules", { contact, rules });
}

const REPORT_ADDRESS = "info@flickertalk.com";

// messages go as evidence only if the user chose to
export function reportLink(contact: string, reason: string, evidence: string[]): string {
  const lines = [`Reported: ${contact}`, `Reason: ${reason}`];
  if (evidence.length) lines.push("", "Evidence:", ...evidence.map((text) => `> ${text}`));
  const subject = encodeURIComponent(`Report ${contact}`);
  return `mailto:${REPORT_ADDRESS}?subject=${subject}&body=${encodeURIComponent(lines.join("\n"))}`;
}

// the name never leaves the phone
export async function renameContact(contact: string, name: string): Promise<void> {
  await invoke("core_rename", { contact, name: name.trim() });
  const chat = store.chats.find((item) => item.id === contact);
  if (chat) chat.name = name.trim();
}

// seconds, 0 = forever / never
export async function setHistory(contact: string, keepFor: number, burnAfterRead: number): Promise<void> {
  await invoke("core_set_history", { contact, keepFor, burnAfterRead });
}

export async function reportContact(contact: string, reason: string, evidence: string[]): Promise<void> {
  await openUrl(reportLink(contact, reason, evidence));
  await block(contact, true);
}

export async function contactDetails(contact: string): Promise<ContactDetails> {
  return invoke<ContactDetails>("core_contact", { contact });
}

export async function block(contact: string, blocked: boolean): Promise<void> {
  await invoke("core_block", { contact, blocked });
  await refreshChats();
}
