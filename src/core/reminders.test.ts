import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import { pendingReminder, remindCancel, remindList, remindSet } from "./reminders";

const NOTES = "com.flickertalk.notes";

describe("reminders", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("sets, cancels and lists a plugin's reminders through the core, for each session", async () => {
    await remindSet(NOTES, "r1", 1_900_000_000_000, "milk", "s1");
    tauri.invoke.mockResolvedValueOnce(true);
    expect(await remindCancel(NOTES, "r1")).toBe(true);
    tauri.invoke.mockResolvedValueOnce([{ plugin: NOTES, id: "r2", at: 1, text: "bread" }]);
    expect(await remindList(NOTES)).toEqual([{ plugin: NOTES, id: "r2", at: 1, text: "bread" }]);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_remind_set", { plugin: NOTES, id: "r1", at: 1_900_000_000_000, text: "milk", session: "s1" }],
      ["core_remind_cancel", { plugin: NOTES, id: "r1", session: undefined }],
      ["core_remind_list", { plugin: NOTES, session: undefined }],
    ]);
  });

  it("says which reminder opened the app, with its session only when it has one", async () => {
    tauri.invoke.mockResolvedValueOnce({ plugin: NOTES, id: "r1", session: null });
    const outside = await pendingReminder();
    expect(outside).toEqual({ plugin: NOTES, id: "r1" });
    expect(outside).not.toHaveProperty("session");
    tauri.invoke.mockResolvedValueOnce({ plugin: NOTES, id: "r2", session: "s1" });
    expect(await pendingReminder()).toEqual({ plugin: NOTES, id: "r2", session: "s1" });
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_pending_reminder");
  });

  it("says no reminder opened the app when the core names no plugin or cannot say", async () => {
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await pendingReminder()).toBeNull();
    tauri.invoke.mockResolvedValueOnce({ plugin: "", id: "r1", session: null });
    expect(await pendingReminder()).toBeNull();
    tauri.invoke.mockRejectedValueOnce(new Error("no bridge"));
    expect(await pendingReminder()).toBeNull();
  });
});
