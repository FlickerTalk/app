import { invoke } from "@tauri-apps/api/core";
import { store } from "./store";

export const TYPING_EVENT = "ft://typing";
// ms
export const TYPING_FADE = 6000;
// ms
export const TYPING_EVERY = 3000;

const typingFades = new Map<string, ReturnType<typeof setTimeout>>();

export function heardTyping(contact: string): void {
  store.typing[contact] = true;
  clearTimeout(typingFades.get(contact));
  typingFades.set(contact, setTimeout(() => quietTyping(contact), TYPING_FADE));
}

export function quietTyping(contact: string): void {
  clearTimeout(typingFades.get(contact));
  typingFades.delete(contact);
  delete store.typing[contact];
}

export function isTyping(contact: string): boolean {
  return store.typing[contact] === true;
}

const typingSent = new Map<string, number>();

// the core sends it only over an open direct connection
export async function sendTyping(contact: string): Promise<void> {
  const now = Date.now();
  if (now - (typingSent.get(contact) ?? 0) < TYPING_EVERY) return;
  typingSent.set(contact, now);
  await invoke("core_typing", { contact }).catch(() => undefined);
}
