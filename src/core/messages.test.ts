import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));

import {
  clearMessageNote,
  deleteForEveryone,
  editMessage,
  forgetMessage,
  forwardMessage,
  markRead,
  pinMessage,
  react,
  resend,
  scheduleText,
  searchMessages,
  sendText,
  setMessageNote,
  shareMessage,
} from "./messages";

describe("messages", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("sends and schedules a text, quoting a message only when answering one, and hands back its id", async () => {
    tauri.invoke.mockResolvedValue("m9");
    expect(await sendText("ft_bob", "hi")).toBe("m9");
    expect(await sendText("ft_bob", "yes", "m1")).toBe("m9");
    expect(await scheduleText("ft_bob", "later", 1_900_000_000_000)).toBe("m9");
    expect(await scheduleText("ft_bob", "later", 1_900_000_000_000, "m1")).toBe("m9");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_send", { contact: "ft_bob", text: "hi" }],
      ["core_send", { contact: "ft_bob", text: "yes", replyTo: "m1" }],
      ["core_schedule", { contact: "ft_bob", text: "later", sendAt: 1_900_000_000_000 }],
      ["core_schedule", { contact: "ft_bob", text: "later", sendAt: 1_900_000_000_000, replyTo: "m1" }],
    ]);
    expect(tauri.invoke.mock.calls[0][1]).not.toHaveProperty("replyTo");
    expect(tauri.invoke.mock.calls[2][1]).not.toHaveProperty("replyTo");
  });

  it("hands the core what the user does with a message", async () => {
    await resend("m1");
    await markRead("ft_bob");
    await editMessage("m1", "  other words ");
    await deleteForEveryone("m1");
    await pinMessage("m1", true);
    await setMessageNote("m1", "com.flickertalk.translate", "hola");
    await clearMessageNote("m1", "com.flickertalk.translate");
    await forgetMessage("m1");
    await forwardMessage("m1", "ft_carol");
    await shareMessage("m1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_resend", { message: "m1" }],
      ["core_mark_read", { contact: "ft_bob" }],
      ["core_edit", { message: "m1", text: "other words" }],
      ["core_delete_everyone", { message: "m1" }],
      ["core_pin", { message: "m1", pinned: true }],
      ["core_message_note_set", { message: "m1", plugin: "com.flickertalk.translate", text: "hola" }],
      ["core_message_note_clear", { message: "m1", plugin: "com.flickertalk.translate" }],
      ["core_forget_message", { message: "m1" }],
      ["core_forward", { message: "m1", contact: "ft_carol" }],
      ["core_share_message", { message: "m1" }],
    ]);
  });

  it("puts an emoji on a message, and takes it back with none", async () => {
    await react("ft_bob", "m1", "👍");
    await react("ft_bob", "m1", null);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_react", { contact: "ft_bob", message: "m1", emoji: "👍" }],
      ["core_react", { contact: "ft_bob", message: "m1" }],
    ]);
    expect(tauri.invoke.mock.calls[1][1]).not.toHaveProperty("emoji");
  });

  it("searches this phone with the words trimmed, and never for nothing", async () => {
    tauri.invoke.mockResolvedValue([{ id: "m9", outgoing: true, text: "friday works", sentAt: 0, state: "read" }]);
    expect(await searchMessages("ft_bob", " friday ")).toEqual([{ id: "m9", mine: true, text: "friday works", time: "", sentAt: 0, status: "read" }]);
    expect(tauri.invoke.mock.calls).toEqual([["core_search", { contact: "ft_bob", query: "friday" }]]);
    tauri.invoke.mockClear();
    expect(await searchMessages("ft_bob", "   ")).toEqual([]);
    expect(tauri.invoke).not.toHaveBeenCalled();
  });
});
