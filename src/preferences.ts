// Privacy choices made in Settings. They stay on this device and never reach a server (Plan §19, §44).

const MAILBOX_KEY = "ft-mailbox";
const CALL_ROUTING_KEY = "ft-call-routing";

// direct: never relay (a call without a direct path simply fails)
// auto:   relay only when the direct connection fails (default)
// always: always relay, which hides your IP from the contact (Plan §17, §67)
export const CALL_ROUTINGS = ["direct", "auto", "always"] as const;
export type CallRouting = (typeof CALL_ROUTINGS)[number];

// The offline mailbox is on by default (Plan §19).
export function storedMailbox(): boolean {
  return localStorage.getItem(MAILBOX_KEY) !== "0";
}

export function setMailbox(enabled: boolean) {
  localStorage.setItem(MAILBOX_KEY, enabled ? "1" : "0");
}

export function storedCallRouting(): CallRouting {
  const value = localStorage.getItem(CALL_ROUTING_KEY);
  return CALL_ROUTINGS.includes(value as CallRouting) ? (value as CallRouting) : "auto";
}

export function setCallRouting(routing: CallRouting) {
  localStorage.setItem(CALL_ROUTING_KEY, routing);
}

const ONBOARDED_KEY = "ft-onboarded";

export function isOnboarded(): boolean {
  return localStorage.getItem(ONBOARDED_KEY) === "1";
}

export function setOnboarded() {
  localStorage.setItem(ONBOARDED_KEY, "1");
}
