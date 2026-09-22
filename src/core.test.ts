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
  it("erases this phone through the core", async () => {
    await core.erasePhone();
    expect(tauri.invoke).toHaveBeenCalledWith("core_erase");
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
