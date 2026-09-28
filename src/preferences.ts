// Privacy choices made in Settings. They stay on this device and never reach a server (Plan §17, §44).
// The mailbox preference lives in the core, which tells contacts about it (Plan §19).
import { invoke } from "@tauri-apps/api/core";

const CALL_ROUTING_KEY = "ft-call-routing";

// direct: never relay (a call without a direct path simply fails)
// auto:   relay only when the direct connection fails (default)
// always: always relay, which hides your IP from the contact (Plan §17, §67)
export const CALL_ROUTINGS = ["direct", "auto", "always"] as const;
export type CallRouting = (typeof CALL_ROUTINGS)[number];

export function storedCallRouting(): CallRouting {
  const value = localStorage.getItem(CALL_ROUTING_KEY);
  return CALL_ROUTINGS.includes(value as CallRouting) ? (value as CallRouting) : "auto";
}

export function setCallRouting(routing: CallRouting) {
  localStorage.setItem(CALL_ROUTING_KEY, routing);
  void syncCallRouting();
}

/**
 * The core keeps a copy of the routing (2026-09-28): a call answered from CallKit on a locked
 * iPhone has no WebView to ask. Sent at start and on every change.
 */
export async function syncCallRouting(): Promise<void> {
  await invoke("core_set_call_routing", { routing: storedCallRouting() }).catch(() => undefined);
}

const ONBOARDED_KEY = "ft-onboarded";

export function isOnboarded(): boolean {
  return localStorage.getItem(ONBOARDED_KEY) === "1";
}

export function setOnboarded() {
  localStorage.setItem(ONBOARDED_KEY, "1");
}

/** After moving to another phone this one is erased, and starts again at the welcome (§60). */
export function clearOnboarded() {
  localStorage.removeItem(ONBOARDED_KEY);
}
