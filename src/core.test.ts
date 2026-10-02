import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string) => `asset://localhost${path}`,
}));
const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers[name] = handler;
    return Promise.resolve(() => undefined);
  },
}));

import * as core from "./core";

const at = (hours: number, minutes: number) => new Date(2026, 8, 22, hours, minutes).getTime();

const answers: Record<string, unknown> = {
  core_me: { id: "ft_me", name: "Ioan", mailbox: true, freeUntil: 1_800_000_000_000 },
  core_conversations: [
    {
      id: "ft_bob",
      name: "Bob",
      unread: 2,
      blocked: false,
      connected: true,
      last: { id: "m2", outgoing: true, text: "see you", sentAt: at(9, 41), state: "delivered" },
    },
    { id: "ft_carol", name: "Carol", unread: 0, blocked: false, connected: false, last: null },
  ],
  core_messages: [
    { id: "m1", outgoing: false, text: "hi", sentAt: at(9, 30), state: "delivered" },
    { id: "m2", outgoing: true, text: "see you", sentAt: at(9, 41), state: "delivered" },
  ],
  core_card: "https://flickertalk.com/add#card",
  core_upload_start: "up1",
  core_add_contact: "ft_dave",
};

describe("core bridge", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(answers[command]));
  });

  it("loads me and the conversations from the core", async () => {
    await core.start();
    expect(core.store.me).toMatchObject({ id: "ft_me", name: "Ioan", mailbox: true, freeUntil: 1_800_000_000_000 });
    expect(core.store.chats.map((chat) => chat.name)).toEqual(["Bob", "Carol"]);
    expect(core.store.chats[0]).toMatchObject({
      unread: 2,
      preview: "see you",
      lastMine: true,
      status: "delivered",
      time: "09:41",
      connected: true,
    });
    expect(core.store.chats[1]).toMatchObject({ preview: "", lastMine: false, time: "", connected: false });
  });

  it("loads a conversation's messages", async () => {
    await core.start();
    await core.loadMessages("ft_bob");
    expect(tauri.invoke).toHaveBeenCalledWith("core_messages", { contact: "ft_bob", limit: 200 });
    expect(core.chat("ft_bob")?.messages).toEqual([
      { id: "m1", mine: false, text: "hi", time: "09:30", sentAt: at(9, 30), status: "delivered" },
      { id: "m2", mine: true, text: "see you", time: "09:41", sentAt: at(9, 41), status: "delivered" },
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

  // Events lost to a burst (2026-10-01): nobody knows what changed, so everything shown reloads.
  it("reloads every open conversation and circle when the core says events were lost", async () => {
    await core.start();
    await core.loadMessages("ft_bob");
    await core.loadCircleMessages("circle1");
    tauri.invoke.mockClear();
    tauri.handlers["ft://changed"]({ payload: { contact: null, all: true } });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_conversations", undefined);
    expect(tauri.invoke).toHaveBeenCalledWith("core_messages", { contact: "ft_bob", limit: 200 });
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_messages", { circle: "circle1", limit: 200 });
  });

  it("forwards the user's intents to the core", async () => {
    await core.sendText("ft_bob", "hello");
    await core.markRead("ft_bob");
    await core.setMailbox(false);
    await core.setName("  Ioan  ");
    await core.enablePush();
    expect(tauri.invoke).toHaveBeenCalledWith("core_enable_push");
    await core.openFile("f1");
    await core.saveFile("f1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_open_file", { message: "f1" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_save_file", { message: "f1" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_send", { contact: "ft_bob", text: "hello" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_mark_read", { contact: "ft_bob" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_mailbox", { enabled: false });
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_name", { name: "Ioan" });
    expect(core.store.me.mailbox).toBe(false);
  });

  // Issues app#4–#6: what this phone takes from a contact and tells them, only in the core.
  it("sets a contact's rules and the receipts default through the core", async () => {
    const rules = { muted: true, acceptsChat: true, acceptsCalls: false, receipts: false };
    await core.setRules("ft_bob", rules);
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_rules", { contact: "ft_bob", rules });
    await core.setReceipts(false);
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_receipts", { enabled: false });
    expect(core.store.me.receipts).toBe(false);
  });

  // Issue app#7: the weekly hours travel as JSON and are off when there are none.
  it("reads and saves the weekly hours", async () => {
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await core.quietHours()).toBeNull();
    const week = { days: ["all", "all", "all", "all", { from: "15:00", to: "22:00" }, "none", "none"] as core.Day[] };
    tauri.invoke.mockResolvedValueOnce(JSON.stringify(week));
    expect(await core.quietHours()).toEqual(week);
    await core.setQuietHours(week);
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_quiet_hours", { hours: JSON.stringify(week) });
    await core.setQuietHours(null);
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_quiet_hours", { hours: null });
  });

  it("adds a contact from a pasted link", async () => {
    expect(await core.addContact("  https://flickertalk.com/add#x  ")).toBe("ft_dave");
    expect(tauri.invoke).toHaveBeenCalledWith("core_add_contact", { link: "https://flickertalk.com/add#x" });
  });

  // §62–63: a file message shows its transfer; an image, once here, shows itself.
  it("loads file messages with their transfer", async () => {
    const file = (id: string, outgoing: boolean, state: string, progress: number) => ({
      id,
      outgoing,
      text: "photo.jpg",
      sentAt: at(9, 50),
      state: "delivered",
      file: { name: "photo.jpg", size: 1_200_000, mime: "image/jpeg", progress, state, path: `/files/${id}/photo.jpg` },
    });
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_messages"
          ? [file("f1", false, "transferring", 0.5), file("f2", false, "done", 1), file("f3", true, "transferring", 0.25)]
          : answers[command],
      ),
    );
    await core.start();
    await core.loadMessages("ft_bob");
    const [receiving, received, sending] = core.chat("ft_bob")?.messages ?? [];
    expect(receiving).toMatchObject({ kind: "file", file: { name: "photo.jpg", size: "1.2 MB", progress: 0.5, state: "receiving" } });
    expect(receiving.file?.url).toBeUndefined();
    expect(received.file).toMatchObject({ state: "done", url: "asset://localhost/files/f2/photo.jpg" });
    expect(sending.file).toMatchObject({ state: "sending", url: "asset://localhost/files/f3/photo.jpg" });
  });

  // Without a direct connection a transfer cannot move: it says so instead of pretending.
  it("shows transfers with a disconnected contact as paused", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_messages"
          ? [{ id: "f1", outgoing: false, text: "a.bin", sentAt: 0, state: "delivered",
              file: { name: "a.bin", size: 10, mime: "", progress: 0.1, state: "transferring", path: "/a" } }]
          : answers[command],
      ),
    );
    await core.start();
    await core.loadMessages("ft_carol");
    expect(core.chat("ft_carol")?.messages[0].file?.state).toBe("paused");
  });

  it("sends a picked file in slices", async () => {
    const file = new File([new Uint8Array(1_200_000)], "report.pdf", { type: "application/pdf" });
    await core.sendFile("ft_bob", file);
    // Android's IPC only carries JSON (no raw bodies), so the bytes travel as base64.
    const appends = tauri.invoke.mock.calls.filter(([command]) => command === "core_upload_append");
    expect(tauri.invoke).toHaveBeenCalledWith("core_upload_start");
    expect(appends).toHaveLength(3);
    expect(appends.every(([, args]) => (args as { upload: string }).upload === "up1")).toBe(true);
    const sizes = appends.map(([, args]) => atob((args as { data: string }).data).length);
    expect(sizes.reduce((total, size) => total + size, 0)).toBe(1_200_000);
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_send_file", {
      contact: "ft_bob",
      upload: "up1",
      name: "report.pdf",
      mime: "application/pdf",
    });
  });

  it("writes sizes the short way", () => {
    expect([0, 512, 1_500, 1_200_000, 48_000_000, 2_300_000_000].map(core.formatSize)).toEqual([
      "0 B",
      "512 B",
      "1.5 KB",
      "1.2 MB",
      "48 MB",
      "2.3 GB",
    ]);
  });

  // §36: a report goes by email, outside the messaging system, and only with what the user
  // chooses to send; the messages as evidence only if they ask for it.
  it("writes a report as an email to FlickerTalk", () => {
    const link = new URL(core.reportLink("ft_bob", "spam", []));
    expect(link.protocol).toBe("mailto:");
    expect(link.pathname).toBe("info@flickertalk.com");
    const body = link.searchParams.get("body") ?? "";
    expect(link.searchParams.get("subject")).toBe("Report ft_bob");
    expect(body).toContain("Reported: ft_bob");
    expect(body).toContain("Reason: spam");
    expect(body).not.toContain("Evidence");
    const withEvidence = new URL(core.reportLink("ft_bob", "abuse", ["you are an idiot"])).searchParams.get("body");
    expect(withEvidence).toContain("you are an idiot");
  });

  it("reports and blocks the contact", async () => {
    await core.reportContact("ft_bob", "spam", []);
    expect(opener.openUrl).toHaveBeenCalledWith(expect.stringMatching(/^mailto:info@flickertalk\.com\?/));
    expect(tauri.invoke).toHaveBeenCalledWith("core_block", { contact: "ft_bob", blocked: true });
  });

  // The list says what the last message was: text, a file or a voice message.
  it("tells what kind of message came last", async () => {
    const file = (mime: string) => ({ name: "x", size: 1, mime, progress: 1, state: "done", path: "/x" });
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_conversations"
          ? [
              { id: "a", name: "A", unread: 0, blocked: false, connected: false, last: { id: "1", outgoing: false, text: "hi", sentAt: 1, state: "read" } },
              { id: "b", name: "B", unread: 0, blocked: false, connected: false, last: { id: "2", outgoing: false, text: "menu.pdf", sentAt: 1, state: "read", file: file("application/pdf") } },
              { id: "c", name: "C", unread: 0, blocked: false, connected: false, last: { id: "3", outgoing: true, text: "voice.m4a", sentAt: 1, state: "read", file: file("audio/mp4") } },
            ]
          : answers[command],
      ),
    );
    await core.refreshChats();
    expect(core.store.chats.map((chat) => chat.lastKind)).toEqual(["text", "file", "voice"]);
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

  // §78: the user can take their device off our server and wipe this phone.
  // A5: strangers who wrote first are listed apart, and a yes or a no goes through the core.
  it("lists the requests apart and answers them through the core", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_requests"
          ? [{ id: "ft_stranger", name: "Someone", unread: 1, blocked: false, connected: false, last: { id: "r1", outgoing: false, text: "hey", sentAt: at(10, 0), state: "delivered" } }]
          : command === "core_conversations" || command === "core_sessions"
            ? []
            : undefined,
      ),
    );
    await core.refreshChats();
    expect(core.store.requests.map((chat) => chat.id)).toEqual(["ft_stranger"]);
    expect(core.store.chats).toEqual([]);
    expect(core.chat("ft_stranger")?.preview).toBe("hey");
    await core.acceptContact("ft_stranger");
    expect(tauri.invoke).toHaveBeenCalledWith("core_accept_contact", { contact: "ft_stranger" });
    await core.declineContact("ft_stranger");
    expect(tauri.invoke).toHaveBeenCalledWith("core_decline_contact", { contact: "ft_stranger" });
  });

  // A5: a new link retires the old one, in the core; A4: what a file that waits needs.
  it("renews the link, asks for waiting files and sets the download limit through the core", async () => {
    tauri.invoke.mockResolvedValue("https://flickertalk.com/add#new");
    expect(await core.renewLink()).toBe("https://flickertalk.com/add#new");
    expect(tauri.invoke).toHaveBeenCalledWith("core_renew_link", {});
    await core.renewLink("s1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_renew_link", { session: "s1" });
    await core.acceptFile("m9");
    expect(tauri.invoke).toHaveBeenCalledWith("core_accept_file", { message: "m9" });
    await core.setAutoDownload(0);
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_auto_download", { bytes: 0 });
    expect(core.store.me.autoDownload).toBe(0);
  });

  // A4: a file that waits for a tap is shown as such, whether or not the contact is connected.
  it("shows a waiting file as waiting, never as paused", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_messages"
          ? [{ id: "f1", outgoing: false, text: "big.zip", sentAt: at(9, 30), state: "delivered", file: { name: "big.zip", size: 50_000_000, mime: "application/zip", progress: 0, state: "waiting", path: "/x" } }]
          : command === "core_conversations"
            ? [{ id: "ft_carol", name: "Carol", unread: 0, blocked: false, connected: false, last: null }]
            : command === "core_sessions" || command === "core_requests"
              ? []
              : undefined,
      ),
    );
    await core.refreshChats();
    await core.loadMessages("ft_carol");
    expect(core.chat("ft_carol")?.messages[0].file?.state).toBe("waiting");
  });

  it("erases this phone through the core", async () => {
    await core.erasePhone();
    expect(tauri.invoke).toHaveBeenCalledWith("core_erase");
  });

  // §78, 2026-09-30: an erased phone starts again like a new install, at the welcome, with
  // nothing this phone chose before (the call routing, the colours).
  it("forgets what this phone chose once it is erased, so the welcome comes again", async () => {
    localStorage.setItem("ft-onboarded", "1");
    localStorage.setItem("ft-call-routing", "always");
    await core.erasePhone();
    expect(localStorage.getItem("ft-onboarded")).toBeNull();
    expect(localStorage.getItem("ft-call-routing")).toBeNull();
  });

  it("keeps what this phone chose when the core could not erase it", async () => {
    localStorage.setItem("ft-onboarded", "1");
    tauri.invoke.mockRejectedValueOnce(new Error("disk"));
    await expect(core.erasePhone()).rejects.toThrow();
    expect(localStorage.getItem("ft-onboarded")).toBe("1");
  });

  // The WebView's own file input leaves the user outside the app on Android; the phone's picker
  // hands back files that are already in the app's folder.
  it("sends what the system picker gave, without passing the bytes through the WebView", async () => {
    const picked = [{ path: "/data/uploads/1-a.jpg", name: "a.jpg", mime: "image/jpeg", size: 12 }];
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_pick_files" ? picked : undefined));

    const files = await core.pickFiles();
    expect(files).toEqual(picked);
    await core.sendPicked("ft_bob", files[0]);
    expect(tauri.invoke).toHaveBeenCalledWith("core_send_picked", { contact: "ft_bob", file: picked[0] });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_upload_start", expect.anything());
  });
});

// Plugins, phase 3 (2026-09-27): records travel as base64 of the plugin's text, and the reminder
// the user tapped comes as `plugin\nid`.
describe("plugin records and reminders", () => {
  beforeEach(() => tauri.invoke.mockReset());

  it("keeps a plugin's text as bytes and brings it back whole", async () => {
    expect(core.fromBase64(core.toBase64Text("milk ñ 🎉"))).toBe("milk ñ 🎉");
    tauri.invoke.mockResolvedValue(core.toBase64Text("{\"a\":1}"));
    expect(await core.pluginRecordGet("com.flickertalk.notes", "note/1")).toBe("{\"a\":1}");
    tauri.invoke.mockResolvedValue(null);
    expect(await core.pluginRecordGet("com.flickertalk.notes", "note/9")).toBeNull();
    await core.pluginRecordSet("com.flickertalk.notes", "note/1", "x");
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_record_set", { plugin: "com.flickertalk.notes", key: "note/1", value: core.toBase64Text("x") });
    tauri.invoke.mockResolvedValue([12, 4096]);
    expect(await core.pluginRecordUsage("com.flickertalk.notes")).toEqual({ used: 12, quota: 4096 });
  });

  // 2026-10-01 (§108): with the hidden session it was set in, if any, so the plugin opens there.
  it("says which reminder opened the app, once", async () => {
    tauri.invoke.mockResolvedValue({ plugin: "com.flickertalk.notes", id: "r1\nstill r1", session: null });
    expect(await core.pendingReminder()).toEqual({ plugin: "com.flickertalk.notes", id: "r1\nstill r1" });
    tauri.invoke.mockResolvedValue({ plugin: "com.flickertalk.notes", id: "r2", session: "s1" });
    expect(await core.pendingReminder()).toEqual({ plugin: "com.flickertalk.notes", id: "r2", session: "s1" });
    tauri.invoke.mockResolvedValue(null);
    expect(await core.pendingReminder()).toBeNull();
    tauri.invoke.mockImplementation(() => Promise.reject(new Error("no bridge")));
    expect(await core.pendingReminder()).toBeNull();
    tauri.invoke.mockReset();
  });
});

// Circles (2026-09-27): the core lists them with the conversations; their messages say who said
// what, and a change in one reloads it if it is on screen.
describe("circles", () => {
  const circles = [
    {
      id: "circle1",
      name: "Friends",
      members: [
        { id: "ft_me", name: "Me", admin: true, me: true },
        { id: "ft_bob", name: "Bob", admin: false, me: false },
      ],
      admin: true,
      adminsOnly: false,
      left: false,
      unread: 2,
      last: { id: "m1", outgoing: false, sender: "ft_bob", senderName: "Bob", kind: "text", text: "dinner?", sentAt: at(10, 2), state: "delivered" },
    },
  ];

  beforeEach(() => {
    core.store.circles = [];
    core.store.sessions = [];
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_circles") return Promise.resolve(circles);
      if (command === "core_circle_messages") {
        return Promise.resolve([
          { id: "e1", outgoing: true, sender: "ft_me", senderName: "Me", kind: "created", text: "Friends", sentAt: at(10, 0), state: "read" },
          { id: "m1", outgoing: false, sender: "ft_bob", senderName: "Bob", kind: "text", text: "dinner?", sentAt: at(10, 2), state: "delivered" },
        ]);
      }
      if (command === "core_circle_create") return Promise.resolve("circle2");
      return Promise.resolve(answers[command]);
    });
  });

  it("lists the circles with the conversations", async () => {
    await core.refreshChats();
    expect(core.store.circles).toHaveLength(1);
    expect(core.store.circles[0]).toMatchObject({ id: "circle1", name: "Friends", unread: 2, preview: "dinner?", time: "10:02", lastMine: false, admin: true });
    expect(core.circle("circle1")?.members.map((member) => member.name)).toEqual(["Me", "Bob"]);
  });

  it("loads a circle's messages, with who said each", async () => {
    await core.refreshChats();
    await core.loadCircleMessages("circle1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_messages", { circle: "circle1", limit: 200 });
    expect(core.circle("circle1")?.messages).toEqual([
      { id: "e1", mine: true, text: "Friends", time: "10:00", status: "read", kind: "created", sender: "ft_me", senderName: "Me" },
      { id: "m1", mine: false, text: "dinner?", time: "10:02", status: "delivered", kind: "text", sender: "ft_bob", senderName: "Bob" },
    ]);
  });

  it("reloads an open circle when the core says it changed", async () => {
    await core.start();
    await core.loadCircleMessages("circle1");
    tauri.invoke.mockClear();
    tauri.handlers[core.CHANGED_EVENT]({ payload: { contact: null, circle: "circle1" } });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_messages", { circle: "circle1", limit: 200 });
  });

  it("hands the core what the user does with a circle", async () => {
    expect(await core.createCircle(" Friends ", ["ft_bob"])).toBe("circle2");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_create", { name: "Friends", members: ["ft_bob"], session: undefined });
    await core.sendCircleText("circle1", "yes");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_send", { circle: "circle1", text: "yes" });
    await core.inviteToCircle("circle1", "ft_carol");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_invite", { circle: "circle1", contact: "ft_carol" });
    await core.leaveCircle("circle1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_circle_leave", { circle: "circle1" });
  });

  it("knows which list a circle lives in, to add people from it", async () => {
    await core.refreshChats();
    expect(core.circleHome("circle1")).toEqual({ contacts: core.store.chats });
    core.store.sessions = [{ id: "s1", chats: [], requests: [], circles: [{ ...core.store.circles[0], id: "circle3" }] }];
    expect(core.circleHome("circle3").session).toBe("s1");
  });

  // 2026-10-01 (§108): a plugin opened from a conversation is open in that conversation's place.
  it("knows which hidden session a conversation lives in", async () => {
    await core.refreshChats();
    const bob = core.store.chats[0];
    core.store.sessions = [{ id: "s1", chats: [{ ...bob, id: "ft_hidden" }], requests: [{ ...bob, id: "ft_stranger" }], circles: [] }];
    expect(core.sessionOf(bob.id)).toBeUndefined();
    expect(core.sessionOf("ft_hidden")).toBe("s1");
    expect(core.sessionOf("ft_stranger")).toBe("s1");
    expect(core.sessionOf("")).toBeUndefined();
  });
});

// Hidden sessions (Plan, 2026-09-23): a 6-digit PIN opens a session of its own, with its own
// contacts and no name. The core answers with its conversations; the same PIN always opens the same one.
describe("hidden sessions", () => {
  beforeEach(() => {
    core.store.sessions = [];
    core.store.chats = [];
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_session_open") {
        return Promise.resolve({
          id: "s1",
          conversations: [
            { id: "ft_pablo", name: "Pablo", unread: 1, blocked: false, connected: true, last: { id: "p1", outgoing: false, text: "Poker?", sentAt: at(13, 30), state: "delivered" } },
          ],
          requests: [],
        });
      }
      return Promise.resolve(undefined);
    });
  });

  // A3: every PIN opens a session in the core, the one that has it or a new empty one. Only with
  // the seven slots taken does the core open nothing: the screen shows an empty session all the
  // same, and a refresh keeps it.
  it("shows an empty session when the core has no room for another", async () => {
    tauri.invoke.mockResolvedValue(null);
    await core.openSession("000000");
    expect(tauri.invoke).toHaveBeenCalledWith("core_session_open", { pin: "000000" });
    expect(core.store.sessions).toEqual([{ id: "", pin: "000000", chats: [], requests: [], circles: [] }]);
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_sessions" ? [] : command === "core_conversations" ? [] : undefined));
    await core.refreshChats();
    expect(core.store.sessions).toHaveLength(1);
  });

  it("closes a session that is only on the screen without telling the core", async () => {
    core.store.sessions = [{ id: "", pin: "000000", chats: [], requests: [], circles: [] }];
    await core.closeSession("");
    expect(tauri.invoke).not.toHaveBeenCalled();
    expect(core.store.sessions).toHaveLength(0);
  });

  it("removes a session through the core", async () => {
    await core.openSession("246810");
    await core.removeSession("s1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_session_remove", { session: "s1" });
    expect(core.store.sessions).toHaveLength(0);
  });

  it("opens a session through the core and lists its conversations", async () => {
    await core.openSession("246810");
    expect(tauri.invoke).toHaveBeenCalledWith("core_session_open", { pin: "246810" });
    expect(core.store.sessions).toHaveLength(1);
    expect(core.store.sessions[0].id).toBe("s1");
    expect(core.store.sessions[0].chats.map((chat) => chat.name)).toEqual(["Pablo"]);
    expect(core.store.sessions[0].chats[0].unread).toBe(1);
  });

  it("finds a session's conversation like any other", async () => {
    await core.openSession("246810");
    expect(core.chat("ft_pablo")?.name).toBe("Pablo");
  });

  // Every change refreshes the open sessions too, so their unread counts follow the main list.
  it("refreshes the open sessions with the conversations", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_conversations") return Promise.resolve(answers.core_conversations);
      if (command === "core_sessions") {
        return Promise.resolve([
          { id: "s1", conversations: [{ id: "ft_pablo", name: "Pablo", unread: 3, blocked: false, connected: false, last: null }], requests: [] },
        ]);
      }
      return Promise.resolve(undefined);
    });
    await core.refreshChats();
    expect(core.store.chats.map((chat) => chat.id)).toEqual(["ft_bob", "ft_carol"]);
    expect(core.store.sessions).toHaveLength(1);
    expect(core.store.sessions[0].chats[0].unread).toBe(3);
  });

  // 2026-10-01 (§108): a session stays open until the user leaves it, across starts. The core
  // restores it, and the chats show its panel at start with no PIN typed.
  it("shows at start the sessions the core kept open, with no PIN", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_me") return Promise.resolve({ id: "ft_me", name: "Me" });
      if (command === "core_conversations") return Promise.resolve([]);
      if (command === "core_sessions") {
        return Promise.resolve([{ id: "s1", conversations: [{ id: "ft_pablo", name: "Pablo", unread: 0, blocked: false, connected: false, last: null }], requests: [] }]);
      }
      return Promise.resolve(undefined);
    });
    await core.start();
    expect(core.store.sessions.map((session) => session.id)).toEqual(["s1"]);
    expect(core.store.sessions[0].chats[0].name).toBe("Pablo");
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_session_open", expect.anything());
  });

  it("adds a contact to a session", async () => {
    tauri.invoke.mockResolvedValue("ft_pablo");
    await core.addContact(" https://flickertalk.com/add#pablo ", "s1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_add_contact", { link: "https://flickertalk.com/add#pablo", session: "s1" });
  });

  it("opens the same session once", async () => {
    await core.openSession("246810");
    await core.openSession("246810");
    expect(core.store.sessions).toHaveLength(1);
  });

  // Leaving hides the session again; the core keeps receiving for it silently.
  it("closes a session through the core and hides it", async () => {
    await core.openSession("246810");
    await core.closeSession("s1");
    expect(tauri.invoke).toHaveBeenCalledWith("core_session_close", { session: "s1" });
    expect(core.store.sessions).toHaveLength(0);
    expect(core.chat("ft_pablo")).toBeUndefined();
  });
});

// §41: one way to count what is left of the free year, for every screen that says it. A day
// that has started counts, so a phone installed a moment ago has the whole 365 days.
describe("the days left", () => {
  const DAY = 24 * 60 * 60 * 1000;
  const now = Date.parse("2026-09-29T10:00:00Z");

  it("counts a year installed a moment ago as 365 days", () => {
    expect(core.daysLeft(now + 365 * DAY - 5, now)).toBe(365);
  });

  it("counts a day that has started as a day", () => {
    expect(core.daysLeft(now + 100.5 * DAY, now)).toBe(101);
    expect(core.daysLeft(now + 1, now)).toBe(1);
  });

  it("never counts below nothing", () => {
    expect(core.daysLeft(now, now)).toBe(0);
    expect(core.daysLeft(now - 3 * DAY, now)).toBe(0);
  });
});

// 2026-09-29: what a year costs comes from the Store, as it formats it, every time it is asked.
describe("the Store's price", () => {
  it("is the Store's own text", async () => {
    tauri.invoke.mockResolvedValueOnce({ price: "0,99 €" });
    expect(await core.subscriptionPrice()).toBe("0,99 €");
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_subscription_price");
  });

  it("is nothing when the Store cannot say or does not answer", async () => {
    tauri.invoke.mockResolvedValueOnce({ price: null });
    expect(await core.subscriptionPrice()).toBeNull();
    tauri.invoke.mockRejectedValueOnce("store_unavailable");
    expect(await core.subscriptionPrice()).toBeNull();
  });
});

// A suggestion from Settings (2026-10-02): what the core says, and "failed" when it cannot say.
describe("a suggestion", () => {
  beforeEach(() => tauri.invoke.mockReset());

  it("goes through the core and says what became of it", async () => {
    for (const word of ["sent", "tooMany", "failed"]) {
      tauri.invoke.mockResolvedValueOnce(word);
      expect(await core.sendFeedback("Stickers, please")).toBe(word);
      expect(tauri.invoke).toHaveBeenLastCalledWith("core_send_feedback", { text: "Stickers, please" });
    }
  });

  // No network, an old router that answers 404, an app with no such command: never a throw.
  it("is failed when the core fails or answers something else", async () => {
    tauri.invoke.mockRejectedValueOnce("cannot reach the router");
    expect(await core.sendFeedback("an idea")).toBe("failed");
    tauri.invoke.mockRejectedValueOnce(new Error("command core_send_feedback not found"));
    expect(await core.sendFeedback("an idea")).toBe("failed");
    tauri.invoke.mockResolvedValueOnce(undefined);
    expect(await core.sendFeedback("an idea")).toBe("failed");
  });
});
