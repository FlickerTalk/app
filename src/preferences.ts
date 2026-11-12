// these choices stay on the device and never reach a server
import { invoke } from "@tauri-apps/api/core";

const CALL_ROUTING_KEY = "ft-call-routing";

// always relays, which hides the IP from the contact
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

// a call answered from CallKit on a locked iPhone has no WebView to ask, so the core keeps a copy
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

export function clearOnboarded() {
  localStorage.removeItem(ONBOARDED_KEY);
}

// installs from before the tour have nothing stored, so it never shows on its own there
const TOUR_KEY = "ft-tour";

export function setTourDue() {
  localStorage.setItem(TOUR_KEY, "due");
}

export function isTourDue(): boolean {
  return localStorage.getItem(TOUR_KEY) === "due";
}

export function setTourSeen() {
  localStorage.setItem(TOUR_KEY, "seen");
}
