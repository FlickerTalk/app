import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export interface PlanView {
  state: "trial" | "subscribed" | "limited";
  // ms, 0 = neither
  until: number;
  // Play keeps renewing, so until is only how long the last check holds. StoreKit's is a real expiry.
  renews?: boolean;
}

const DAY = 24 * 60 * 60 * 1000;

export function daysLeft(until: number, now = Date.now()): number {
  return Math.max(0, Math.ceil((until - now) / DAY));
}

export async function plan(): Promise<PlanView> {
  return invoke<PlanView>("core_plan");
}

export const PREMIUM_PAGE = "/tabs/settings#premium";

export const PLAN_EVENT = "ft://plan";

export async function followPlan(changed: () => void): Promise<() => void> {
  return listen(PLAN_EVENT, () => changed()).catch(() => () => undefined);
}

export const NEEDS_SUBSCRIPTION = "needs_subscription";

export function needsSubscription(error: unknown): boolean {
  return String(error instanceof Error ? error.message : error) === NEEDS_SUBSCRIPTION;
}

// unknown Store errors show as a plain failure, never as they came
export function payTrouble(error: unknown): string {
  const answer = String(error).trim();
  if (answer === "cancelled") return "";
  const known = ["not_on_sale", "store_unavailable", "pending_approval"];
  return `plan.trouble.${known.includes(answer) ? answer : "failed"}`;
}

// never cached, a price from another store or country must not show
export async function subscriptionPrice(): Promise<string | null> {
  try {
    const answer = await invoke<{ price: string | null } | null>("core_subscription_price");
    return answer?.price?.trim() || null;
  } catch {
    return null;
  }
}

export async function subscribe(): Promise<void> {
  await invoke("core_subscribe");
}

export type RestoreOutcome = "restored" | "nothing";

export async function restoreSubscription(): Promise<RestoreOutcome> {
  return (await invoke<string>("core_restore_subscription")) === "restored" ? "restored" : "nothing";
}
