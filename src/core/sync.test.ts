import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers[name] = handler;
    return Promise.resolve(() => undefined);
  },
}));

import { hueOf } from "./format";
import { store } from "./store";
import { CHANGED_EVENT, loadCircleMessages, loadMessages, refreshChats, start } from "./sync";
import { isTyping, TYPING_EVENT } from "./typing";

const me = { id: "ft_me", name: "Alice", mailbox: false, receipts: true, freeUntil: 0, autoDownload: 0 };

const answers: Record<string, unknown> = {
  core_me: me,
  core_support_id: "ft_support",
  core_conversations: [{ id: "ft_bob", name: "Bob", unread: 0, blocked: false, connected: true, last: null }],
  core_messages: [{ id: "m1", outgoing: false, text: "hi", sentAt: 1, state: "read" }],
  core_circles: [{ id: "circle1", name: "Friends", members: [], admin: true, adminsOnly: false, left: false, unread: 0, last: null }],
  core_circle_messages: [{ id: "c1", outgoing: true, sender: "ft_me", senderName: "Alice", kind: "text", text: "yes", sentAt: 1, state: "sent" }],
};

const commands = () => tauri.invoke.mock.calls.map(([command]) => command);

const LISTS = ["core_conversations", "core_requests", "core_circles", "core_sessions"];

describe("keeping the store in step with the core", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(answers[command]));
  });

  it("starts with me, the support contact and every list, and then is ready", async () => {
    await start();
    expect(store.me).toEqual({ ...me, hue: hueOf("ft_me") });
    expect(store.supportId).toBe("ft_support");
    expect(store.chats.map((one) => one.id)).toEqual(["ft_bob"]);
    expect(store.circles.map((one) => one.id)).toEqual(["circle1"]);
    expect(store.ready).toBe(true);
    expect(Object.keys(tauri.handlers).sort()).toEqual([CHANGED_EVENT, TYPING_EVENT].sort());
  });

  it("starts without a support contact when the core cannot name one", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      command === "core_support_id" ? Promise.reject(new Error("no card")) : Promise.resolve(answers[command]),
    );
    await start();
    expect(store.supportId).toBe("");
  });

  it("takes a list the core does not give as empty", async () => {
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_conversations" ? [] : undefined));
    await refreshChats();
    expect([store.chats, store.requests, store.circles, store.sessions]).toEqual([[], [], [], []]);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_conversations", undefined],
      ["core_requests", {}],
      ["core_circles", undefined],
      ["core_sessions", undefined],
    ]);
  });

  it("puts the sessions only on the screen after the ones the core keeps", async () => {
    store.sessions = [{ id: "", pin: "000000", chats: [], requests: [], circles: [] }];
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_sessions" ? [{ id: "s1", conversations: [], requests: [] }] : command === "core_conversations" ? [] : undefined),
    );
    await refreshChats();
    expect(store.sessions.map((session) => [session.id, session.pin])).toEqual([
      ["s1", undefined],
      ["", "000000"],
    ]);
  });

  it("loads a conversation's last messages into it, with its connection", async () => {
    await refreshChats();
    await loadMessages("ft_bob");
    expect(tauri.invoke).toHaveBeenCalledWith("core_messages", { contact: "ft_bob", limit: 200 });
    expect(store.chats[0].messages.map((message) => message.id)).toEqual(["m1"]);
  });

  it("loads a circle's last messages into it", async () => {
    await refreshChats();
    await loadCircleMessages("circle1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_messages", { circle: "circle1", limit: 200 });
    expect(store.circles[0].messages.map((message) => message.id)).toEqual(["c1"]);
  });

  it("loads nothing for a conversation or a circle the store does not have", async () => {
    await expect(loadMessages("ft_nobody")).resolves.toBeUndefined();
    await expect(loadCircleMessages("circle9")).resolves.toBeUndefined();
  });

  it("reloads only the lists when something not open changes", async () => {
    await start();
    tauri.invoke.mockClear();
    tauri.handlers[CHANGED_EVENT]({ payload: { contact: "ft_unopened" } });
    await flushPromises();
    expect(commands()).toEqual(LISTS);
    tauri.invoke.mockClear();
    tauri.handlers[CHANGED_EVENT]({ payload: { contact: null, circle: "circle_unopened" } });
    await flushPromises();
    expect(commands()).toEqual(LISTS);
  });

  it("reloads the lists before an open conversation, whose files depend on the connection", async () => {
    await start();
    await loadMessages("ft_bob");
    tauri.invoke.mockClear();
    tauri.handlers[CHANGED_EVENT]({ payload: { contact: "ft_bob" } });
    await flushPromises();
    expect(commands()).toEqual([...LISTS, "core_messages"]);
  });

  it("stops showing a contact typing when something of theirs arrives", async () => {
    await start();
    tauri.handlers[TYPING_EVENT]({ payload: { contact: "ft_bob" } });
    expect(isTyping("ft_bob")).toBe(true);
    tauri.handlers[CHANGED_EVENT]({ payload: { contact: "ft_bob" } });
    expect(isTyping("ft_bob")).toBe(false);
  });
});
