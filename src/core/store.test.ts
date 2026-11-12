import { beforeEach, describe, expect, it } from "vitest";
import { chat, circle, circleHome, requestPending, sessionOf, store, type Chat, type Circle } from "./store";

const someone = (id: string): Chat => ({
  id,
  name: id,
  hue: 0,
  connected: false,
  unread: 0,
  time: "",
  preview: "",
  lastMine: false,
  lastKind: "text",
  status: "",
  blocked: false,
  messages: [],
});

const group = (id: string): Circle => ({
  id,
  name: id,
  hue: 0,
  members: [],
  admin: false,
  adminsOnly: false,
  left: false,
  unread: 0,
  time: "",
  preview: "",
  lastMine: false,
  lastSender: "",
  status: "",
  messages: [],
});

describe("the store", () => {
  it("starts empty, with the mailbox and the receipts on, until the core answers", () => {
    expect(store.ready).toBe(false);
    expect(store.me).toEqual({ id: "", name: "", hue: 0, mailbox: true, receipts: true, freeUntil: 0, autoDownload: 0 });
    expect([store.chats, store.requests, store.sessions, store.circles]).toEqual([[], [], [], []]);
    expect(store.typing).toEqual({});
    expect(store.supportId).toBe("");
  });
});

describe("finding things in the store", () => {
  beforeEach(() => {
    store.chats = [someone("ft_bob")];
    store.requests = [someone("ft_stranger")];
    store.circles = [group("circle1")];
    store.sessions = [{ id: "s1", chats: [someone("ft_hidden")], requests: [someone("ft_shy")], circles: [group("circle2")] }];
  });

  it("finds a conversation in the main list, the requests and every session", () => {
    expect(["ft_bob", "ft_stranger", "ft_hidden", "ft_shy"].map((id) => chat(id)?.id)).toEqual(["ft_bob", "ft_stranger", "ft_hidden", "ft_shy"]);
    expect(chat("ft_nobody")).toBeUndefined();
  });

  it("says a stranger's request is waiting, also inside a session", () => {
    expect(["ft_stranger", "ft_shy", "ft_bob", "ft_hidden"].map(requestPending)).toEqual([true, true, false, false]);
  });

  it("finds a circle in the main list or in a session", () => {
    expect(circle("circle1")?.id).toBe("circle1");
    expect(circle("circle2")?.id).toBe("circle2");
    expect(circle("circle9")).toBeUndefined();
  });

  it("knows which list a circle lives in, to add people from it", () => {
    expect(circleHome("circle1")).toEqual({ contacts: store.chats });
    expect(circleHome("circle2")).toEqual({ session: "s1", contacts: store.sessions[0].chats });
  });

  it("knows which hidden session a conversation or a request lives in", () => {
    expect(sessionOf("ft_hidden")).toBe("s1");
    expect(sessionOf("ft_shy")).toBe("s1");
    expect(sessionOf("ft_bob")).toBeUndefined();
    expect(sessionOf("")).toBeUndefined();
  });

  it("places no conversation in a session that is only on the screen", () => {
    store.sessions = [{ id: "", pin: "000000", chats: [someone("ft_ghost")], requests: [], circles: [] }];
    expect(sessionOf("ft_ghost")).toBeUndefined();
  });
});
