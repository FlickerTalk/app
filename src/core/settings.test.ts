import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  enablePush,
  erasePhone,
  quietHours,
  resumeRouter,
  sendFeedback,
  setMailbox,
  setName,
  setQuietHours,
  setReceipts,
  shareText,
  type Week,
} from "./settings";
import { store } from "./store";

describe("this phone's settings", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
    localStorage.clear();
  });

  it("sets my trimmed name, the mailbox and the receipts through the core, and remembers them", async () => {
    await setName("  Alice ");
    await setMailbox(false);
    await setReceipts(false);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_set_name", { name: "Alice" }],
      ["core_set_mailbox", { enabled: false }],
      ["core_set_receipts", { enabled: false }],
    ]);
    expect(store.me).toMatchObject({ name: "Alice", mailbox: false, receipts: false });
  });

  it("keeps what was set when the core refuses a change", async () => {
    store.me.name = "Alice";
    store.me.mailbox = true;
    tauri.invoke.mockRejectedValue(new Error("disk"));
    await expect(setName("Bob")).rejects.toThrow();
    await expect(setMailbox(false)).rejects.toThrow();
    expect(store.me).toMatchObject({ name: "Alice", mailbox: true });
  });

  it("reads the weekly hours the core keeps as text, and saves them the same way", async () => {
    const week: Week = { days: ["all", "none", { from: "09:00", to: "17:00" }, "all", "all", "all", "all"] };
    tauri.invoke.mockResolvedValueOnce(JSON.stringify(week));
    expect(await quietHours()).toEqual(week);
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await quietHours()).toBeNull();
    await setQuietHours(week);
    await setQuietHours(null);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_quiet_hours"],
      ["core_quiet_hours"],
      ["core_set_quiet_hours", { hours: JSON.stringify(week) }],
      ["core_set_quiet_hours", { hours: null }],
    ]);
  });

  it("turns push on and wakes the router without ever failing", async () => {
    tauri.invoke.mockRejectedValue(new Error("no core"));
    await expect(enablePush()).resolves.toBeUndefined();
    await expect(resumeRouter()).resolves.toBeUndefined();
    expect(tauri.invoke.mock.calls).toEqual([["core_enable_push"], ["core_resume"]]);
  });

  it("shares a text through the system", async () => {
    await shareText("https://flickertalk.com/add#card");
    expect(tauri.invoke.mock.calls).toEqual([["core_share", { text: "https://flickertalk.com/add#card" }]]);
  });

  it("forgets what this phone chose only once the core has erased it", async () => {
    localStorage.setItem("ft-onboarded", "1");
    tauri.invoke.mockRejectedValueOnce(new Error("disk"));
    await expect(erasePhone()).rejects.toThrow();
    expect(localStorage.getItem("ft-onboarded")).toBe("1");
    await erasePhone();
    expect(localStorage.length).toBe(0);
    expect(tauri.invoke.mock.calls).toEqual([["core_erase"], ["core_erase"]]);
  });

  it("says what became of a suggestion, and failed for anything else", async () => {
    for (const outcome of ["sent", "tooMany", "failed"]) {
      tauri.invoke.mockResolvedValueOnce(outcome);
      expect(await sendFeedback("Stickers, please")).toBe(outcome);
    }
    tauri.invoke.mockResolvedValueOnce("queued");
    expect(await sendFeedback("an idea")).toBe("failed");
    tauri.invoke.mockRejectedValueOnce("offline");
    expect(await sendFeedback("an idea")).toBe("failed");
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_send_feedback", { text: "an idea" });
  });
});
