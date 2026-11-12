import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

import {
  daysLeft,
  followPlan,
  NEEDS_SUBSCRIPTION,
  needsSubscription,
  payTrouble,
  plan,
  PLAN_EVENT,
  restoreSubscription,
  subscribe,
  subscriptionPrice,
} from "./billing";

const DAY = 24 * 60 * 60 * 1000;

describe("the plan", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.listen.mockReset();
  });

  it("asks the core for the plan and to subscribe", async () => {
    tauri.invoke.mockResolvedValueOnce({ state: "trial", until: 1_900_000_000_000 });
    expect(await plan()).toEqual({ state: "trial", until: 1_900_000_000_000 });
    await subscribe();
    expect(tauri.invoke.mock.calls).toEqual([["core_plan"], ["core_subscribe"]]);
  });

  it("follows the plan's changes, and follows nothing when it cannot listen", async () => {
    const stop = () => undefined;
    const changed = vi.fn();
    tauri.listen.mockResolvedValueOnce(stop);
    expect(await followPlan(changed)).toBe(stop);
    expect(tauri.listen).toHaveBeenCalledWith(PLAN_EVENT, expect.any(Function));
    tauri.listen.mock.calls[0][1]({ payload: null });
    expect(changed).toHaveBeenCalledOnce();
    tauri.listen.mockRejectedValueOnce(new Error("no bridge"));
    const nothing = await followPlan(changed);
    expect(nothing()).toBeUndefined();
  });

  it("restores only what the Store says it restored", async () => {
    tauri.invoke.mockResolvedValueOnce("restored");
    expect(await restoreSubscription()).toBe("restored");
    tauri.invoke.mockResolvedValueOnce("none");
    expect(await restoreSubscription()).toBe("nothing");
    expect(tauri.invoke.mock.calls).toEqual([["core_restore_subscription"], ["core_restore_subscription"]]);
  });

  it("shows the Store's own price, and none when it has only spaces or nothing", async () => {
    tauri.invoke.mockResolvedValueOnce({ price: " 0,99 € " });
    expect(await subscriptionPrice()).toBe("0,99 €");
    tauri.invoke.mockResolvedValueOnce({ price: "  " });
    expect(await subscriptionPrice()).toBeNull();
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await subscriptionPrice()).toBeNull();
  });

  it("says nothing of a cancelled payment, names the known Store troubles and calls the rest a failure", () => {
    expect(payTrouble(" cancelled ")).toBe("");
    expect(["not_on_sale", "store_unavailable", "pending_approval"].map((answer) => payTrouble(answer))).toEqual([
      "plan.trouble.not_on_sale",
      "plan.trouble.store_unavailable",
      "plan.trouble.pending_approval",
    ]);
    expect(payTrouble(new Error("store_unavailable"))).toBe("plan.trouble.failed");
    expect(payTrouble("something new")).toBe("plan.trouble.failed");
  });

  it("knows the core's refusal of a locked tool, as text or as an error", () => {
    expect(needsSubscription(NEEDS_SUBSCRIPTION)).toBe(true);
    expect(needsSubscription(new Error(NEEDS_SUBSCRIPTION))).toBe(true);
    expect(needsSubscription(new Error("not offered here"))).toBe(false);
  });

  it("counts a started day as a whole one and never goes below nothing", () => {
    const now = Date.parse("2026-09-29T10:00:00Z");
    expect([now + 15 * DAY, now + 1, now, now - DAY].map((until) => daysLeft(until, now))).toEqual([15, 1, 0, 0]);
  });
});
