import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => `asset://localhost${path}` }));

import { hueOf } from "./format";
import { store } from "./store";
import {
  toChat,
  toCircle,
  toCircleMessage,
  toMessage,
  toSession,
  type CircleMessageView,
  type CircleView,
  type ConversationView,
  type MessageView,
} from "./views";

const at = (hours: number, minutes: number) => new Date(2026, 8, 22, hours, minutes).getTime();

const said = (id: string, outgoing = false): MessageView => ({ id, outgoing, text: "hi", sentAt: at(9, 30), state: "delivered" });

const file = (state: "transferring" | "waiting" | "done" | "failed", mime = "image/jpeg") => ({
  name: "a",
  size: 2_500,
  mime,
  progress: 0.5,
  state,
  path: "/files/a",
});

beforeEach(() => {
  store.chats = [];
  store.requests = [];
  store.circles = [];
  store.sessions = [];
});

describe("a message from the core", () => {
  it("carries only what it has", () => {
    expect(toMessage(said("m1"))).toEqual({ id: "m1", mine: false, text: "hi", time: "09:30", sentAt: at(9, 30), status: "delivered" });
  });

  it("keeps its quote, reactions, pin, edit, removal, schedule, waiting change and notes", () => {
    const extras = {
      quote: { id: "m0", text: "dinner?", mine: false, kind: "text" as const },
      reactions: { theirs: "👍" },
      pinned: true,
      edited: true,
      deleted: true,
      scheduledFor: at(10, 0),
      updatePending: true,
      notes: [{ plugin: "com.flickertalk.translate", text: "hola" }],
    };
    expect(toMessage({ ...said("m1", true), ...extras })).toEqual({
      id: "m1",
      mine: true,
      text: "hi",
      time: "09:30",
      sentAt: at(9, 30),
      status: "delivered",
      ...extras,
    });
  });

  it("leaves out an empty list of notes", () => {
    expect(toMessage({ ...said("m1"), notes: [] })).not.toHaveProperty("notes");
  });
});

describe("a file from the core", () => {
  it("shows the transfer as the connection allows", () => {
    const states = [
      toMessage({ ...said("f1", true), file: file("transferring") }, true),
      toMessage({ ...said("f2"), file: file("transferring") }, true),
      toMessage({ ...said("f3"), file: file("transferring") }, false),
      toMessage({ ...said("f4"), file: file("waiting") }, true),
      toMessage({ ...said("f5"), file: file("failed") }, false),
      toMessage({ ...said("f6"), file: file("done") }),
    ].map((message) => message.file?.state);
    expect(states).toEqual(["sending", "receiving", "paused", "waiting", "failed", "done"]);
  });

  it("gives a url only to a picture, a sound or a video that is mine or whole", () => {
    const url = (view: MessageView) => toMessage(view, true).file?.url;
    expect(url({ ...said("f1", true), file: file("transferring") })).toBe("asset://localhost/files/a");
    expect(url({ ...said("f2"), file: file("transferring") })).toBeUndefined();
    expect(url({ ...said("f3"), file: file("done", "audio/mp4") })).toBe("asset://localhost/files/a");
    expect(url({ ...said("f4", true), file: file("done", "application/pdf") })).toBeUndefined();
  });

  it("writes the size short and keeps the exact bytes", () => {
    expect(toMessage({ ...said("f1"), file: file("done") })).toMatchObject({
      kind: "file",
      file: { name: "a", size: "2.5 KB", bytes: 2_500, mime: "image/jpeg", progress: 0.5 },
    });
  });
});

describe("a conversation from the core", () => {
  const carol = (last: MessageView | null = null): ConversationView => ({ id: "ft_carol", name: "Carol", unread: 1, blocked: false, connected: true, last });

  it("is empty until something is said", () => {
    expect(toChat(carol())).toEqual({
      id: "ft_carol",
      name: "Carol",
      hue: hueOf("ft_carol"),
      connected: true,
      unread: 1,
      time: "",
      preview: "",
      lastMine: false,
      lastKind: "text",
      status: "",
      blocked: false,
      messages: [],
    });
  });

  it("shows the last message and tells a voice message from another file", () => {
    expect(toChat(carol(said("m1", true)))).toMatchObject({ time: "09:30", preview: "hi", lastMine: true, lastKind: "text", status: "delivered" });
    expect(toChat(carol({ ...said("m2"), file: file("done", "audio/mp4") })).lastKind).toBe("voice");
    expect(toChat(carol({ ...said("m3"), file: file("done", "application/pdf") })).lastKind).toBe("file");
  });

  it("keeps the messages already loaded for it", () => {
    const loaded = toChat(carol());
    loaded.messages = [toMessage(said("m1"))];
    store.chats = [loaded];
    expect(toChat(carol()).messages.map((message) => message.id)).toEqual(["m1"]);
  });
});

describe("a circle from the core", () => {
  const line = (kind: CircleMessageView["kind"], outgoing: boolean, notes?: CircleMessageView["notes"]): CircleMessageView => ({
    id: "c1",
    outgoing,
    sender: "ft_bob",
    senderName: "Bob",
    kind,
    text: "dinner?",
    sentAt: at(10, 2),
    state: "delivered",
    ...(notes ? { notes } : {}),
  });
  const friends = (last: CircleMessageView | null): CircleView => ({
    id: "circle1",
    name: "Friends",
    members: [],
    admin: false,
    adminsOnly: true,
    left: false,
    unread: 1,
    last,
  });

  it("names who said the last text, unless it was me or something that happened", () => {
    expect(toCircle(friends(line("text", false)))).toMatchObject({ lastSender: "Bob", preview: "dinner?", time: "10:02", adminsOnly: true });
    expect(toCircle(friends(line("text", true))).lastSender).toBe("");
    expect(toCircle(friends(line("joined", false))).lastSender).toBe("");
    expect(toCircle(friends(null))).toMatchObject({ lastSender: "", time: "", preview: "", status: "", hue: hueOf("circle1") });
  });

  it("keeps the messages already loaded for it", () => {
    const loaded = toCircle(friends(null));
    loaded.messages = [toCircleMessage(line("text", false))];
    store.circles = [loaded];
    expect(toCircle(friends(null)).messages.map((message) => message.id)).toEqual(["c1"]);
  });

  it("shows who said each line, with the notes tools left only when there are some", () => {
    expect(toCircleMessage(line("text", false, []))).toEqual({
      id: "c1",
      mine: false,
      text: "dinner?",
      time: "10:02",
      status: "delivered",
      kind: "text",
      sender: "ft_bob",
      senderName: "Bob",
    });
    expect(toCircleMessage(line("text", false, [{ plugin: "p", text: "n" }])).notes).toEqual([{ plugin: "p", text: "n" }]);
  });
});

describe("a session from the core", () => {
  it("has no circles when the core lists none", () => {
    expect(toSession({ id: "s1", conversations: [], requests: [] })).toEqual({ id: "s1", chats: [], requests: [], circles: [] });
  });
});
