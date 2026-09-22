import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers[name] = handler;
    return Promise.resolve(() => undefined);
  },
}));

import * as core from "./core";

const at = (hours: number, minutes: number) => new Date(2026, 8, 22, hours, minutes).getTime();

const answers: Record<string, unknown> = {
  core_me: { id: "ft_me", name: "Ioan", mailbox: true },
  core_conversations: [
    {
      id: "ft_bob",
      name: "Bob",
      unread: 2,
      blocked: false,
      last: { id: "m2", outgoing: true, text: "see you", sentAt: at(9, 41), state: "delivered" },
    },
    { id: "ft_carol", name: "Carol", unread: 0, blocked: false, last: null },
  ],
  core_messages: [
    { id: "m1", outgoing: false, text: "hi", sentAt: at(9, 30), state: "delivered" },
    { id: "m2", outgoing: true, text: "see you", sentAt: at(9, 41), state: "delivered" },
  ],
  core_card: "https://flickertalk.com/add#card",
  core_add_contact: "ft_dave",
};

describe("core bridge", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(answers[command]));
  });

  it("loads me and the conversations from the core", async () => {
    await core.start();
    expect(core.store.me).toMatchObject({ id: "ft_me", name: "Ioan", mailbox: true });
    expect(core.store.chats.map((chat) => chat.name)).toEqual(["Bob", "Carol"]);
    expect(core.store.chats[0]).toMatchObject({
      unread: 2,
      preview: "see you",
      lastMine: true,
      status: "delivered",
      time: "09:41",
    });
    expect(core.store.chats[1]).toMatchObject({ preview: "", lastMine: false, time: "" });
  });

  it("loads a conversation's messages", async () => {
    await core.start();
    await core.loadMessages("ft_bob");
    expect(tauri.invoke).toHaveBeenCalledWith("core_messages", { contact: "ft_bob", limit: 200 });
    expect(core.chat("ft_bob")?.messages).toEqual([
      { id: "m1", mine: false, text: "hi", time: "09:30", status: "delivered" },
      { id: "m2", mine: true, text: "see you", time: "09:41", status: "delivered" },
    ]);
  });

  // The core announces changes; the list and any open conversation follow.
  it("refreshes when the core announces a change", async () => {
    await core.start();
    await core.loadMessages("ft_bob");
    tauri.invoke.mockClear();
    tauri.handlers["ft://changed"]({ payload: { contact: "ft_bob" } });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_conversations", undefined);
    expect(tauri.invoke).toHaveBeenCalledWith("core_messages", { contact: "ft_bob", limit: 200 });
  });

  it("forwards the user's intents to the core", async () => {
    await core.sendText("ft_bob", "hello");
    await core.markRead("ft_bob");
    await core.setMailbox(false);
    await core.setName("  Ioan  ");
    expect(tauri.invoke).toHaveBeenCalledWith("core_send", { contact: "ft_bob", text: "hello" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_mark_read", { contact: "ft_bob" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_mailbox", { enabled: false });
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_name", { name: "Ioan" });
    expect(core.store.me.mailbox).toBe(false);
  });

  it("adds a contact from a pasted link", async () => {
    expect(await core.addContact("  https://flickertalk.com/add#x  ")).toBe("ft_dave");
    expect(tauri.invoke).toHaveBeenCalledWith("core_add_contact", { link: "https://flickertalk.com/add#x" });
  });

  it("gives every contact a stable colour", () => {
    expect(core.hueOf("ft_bob")).toBe(core.hueOf("ft_bob"));
    expect(core.hueOf("ft_bob")).toBeGreaterThanOrEqual(0);
    expect(core.hueOf("ft_bob")).toBeLessThan(360);
    expect(core.hueOf("ft_bob")).not.toBe(core.hueOf("ft_carol"));
  });

  it("shows times as hours and minutes", () => {
    expect(core.clock(at(7, 5))).toBe("07:05");
    expect(core.clock(0)).toBe("");
  });
});
