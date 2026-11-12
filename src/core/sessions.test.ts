import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke, convertFileSrc: (path: string) => path }));

import { closeSession, openSession, removeSession } from "./sessions";
import { store } from "./store";

const pablo = {
  id: "s1",
  conversations: [{ id: "ft_pablo", name: "Pablo", unread: 1, blocked: false, connected: false, last: null }],
  requests: [],
};

describe("hidden sessions", () => {
  beforeEach(() => {
    store.sessions = [];
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("shows what the core opened for a PIN", async () => {
    tauri.invoke.mockResolvedValue(pablo);
    await openSession("246810");
    expect(tauri.invoke).toHaveBeenCalledWith("core_session_open", { pin: "246810" });
    expect(store.sessions.map((session) => [session.id, session.chats.map((chat) => chat.id), session.circles])).toEqual([["s1", ["ft_pablo"], []]]);
  });

  it("shows the session again with what it has now when its PIN is entered twice", async () => {
    tauri.invoke.mockResolvedValue(pablo);
    await openSession("246810");
    tauri.invoke.mockResolvedValue({ ...pablo, conversations: [] });
    await openSession("246810");
    expect(store.sessions).toHaveLength(1);
    expect(store.sessions[0].chats).toEqual([]);
  });

  it("shows an empty session for each PIN when the core has no room", async () => {
    tauri.invoke.mockResolvedValue(null);
    await openSession("111111");
    await openSession("222222");
    await openSession("111111");
    expect(store.sessions).toEqual([
      { id: "", pin: "111111", chats: [], requests: [], circles: [] },
      { id: "", pin: "222222", chats: [], requests: [], circles: [] },
    ]);
  });

  it("closes a session through the core, and one only on the screen without it", async () => {
    store.sessions = [
      { id: "", pin: "111111", chats: [], requests: [], circles: [] },
      { id: "s1", chats: [], requests: [], circles: [] },
    ];
    await closeSession("");
    expect(tauri.invoke).not.toHaveBeenCalled();
    await closeSession("s1");
    expect(tauri.invoke.mock.calls).toEqual([["core_session_close", { session: "s1" }]]);
    expect(store.sessions).toEqual([]);
  });

  it("keeps a session on the screen when the core cannot close or remove it", async () => {
    store.sessions = [{ id: "s1", chats: [], requests: [], circles: [] }];
    tauri.invoke.mockRejectedValue(new Error("locked"));
    await expect(closeSession("s1")).rejects.toThrow();
    await expect(removeSession("s1")).rejects.toThrow();
    expect(store.sessions.map((session) => session.id)).toEqual(["s1"]);
  });

  it("removes a session through the core", async () => {
    store.sessions = [{ id: "s1", chats: [], requests: [], circles: [] }];
    await removeSession("s1");
    expect(tauri.invoke.mock.calls).toEqual([["core_session_remove", { session: "s1" }]]);
    expect(store.sessions).toEqual([]);
  });
});
