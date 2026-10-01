import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

import { fromFrame, frameUrl, installed, openersOf, opensKind, refreshPlugins, viewerOf } from "./plugins";
import type { PluginView } from "./core";

const CODE: PluginView = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
};
const LOCKED: PluginView = { ...CODE, id: "com.example.locked", granted: { network: [], messages: false, send: "nothing" } };

describe("plugins in the app", () => {
  beforeEach(() => tauri.invoke.mockReset());

  // The apps of this phone are all the ones installed: a tool that works on a picture never
  // needed to read a message (§53). What was granted decides what it is handed, not whether it is
  // there.
  it("offers every app that is installed", async () => {
    tauri.invoke.mockResolvedValue([CODE, LOCKED]);
    expect((await refreshPlugins()).map((plugin) => plugin.id)).toEqual([CODE.id, LOCKED.id]);
  });

  // One list for the whole app: what Settings installs or removes is what a conversation sees.
  it("keeps the list where every screen reads it", async () => {
    tauri.invoke.mockResolvedValue([CODE]);
    await refreshPlugins();
    expect(installed.value.map((plugin) => plugin.id)).toEqual([CODE.id]);

    tauri.invoke.mockResolvedValue([]);
    await refreshPlugins();
    expect(installed.value).toEqual([]);
  });

  it("serves each plugin from its own place, never from the app's", () => {
    const url = frameUrl("com.flickertalk.code");
    expect(url).toBe("http://ftplugin.localhost/com.flickertalk.code/frame.html");
    expect(url).not.toContain("tauri.localhost");
  });

  // Anything that is not this plugin's own frame is ignored, whatever it says.
  it("listens only to the frame of the plugin", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const good = { source: frame.contentWindow, data: { type: "ft.height", height: 120 } } as unknown as MessageEvent;
    expect(fromFrame(good, frame)).toEqual({ type: "ft.height", height: 120 });

    const elsewhere = { source: {}, data: { type: "ft.height", height: 1 } } as unknown as MessageEvent;
    expect(fromFrame(elsewhere, frame)).toBeNull();

    const nonsense = { source: frame.contentWindow, data: { type: "ft.doSomething" } } as unknown as MessageEvent;
    expect(fromFrame(nonsense, frame)).toBeNull();
  });

  // What a plugin may say to the app, and nothing else (§53): give me a file, take this file,
  // put this text in the chat, this is how tall I am.
  it("understands only what a plugin is allowed to ask", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.ready" })).toEqual({ type: "ft.ready" });
    expect(said({ type: "ft.pickFile", id: "q1", accept: "image/*" })).toEqual({
      type: "ft.pickFile",
      id: "q1",
      accept: "image/*",
    });
    expect(said({ type: "ft.made", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toEqual({
      type: "ft.made",
      name: "a.pdf",
      mime: "application/pdf",
      data: "AAA",
    });
    expect(said({ type: "ft.text", text: "hello" })).toEqual({ type: "ft.text", text: "hello" });
    expect(said({ type: "ft.readEverything" })).toBeNull();
    expect(said({ type: "ft.made" })).toBeNull();
  });

  // The rest of what the core exposes (issue app#4): the phone, the network and a memory of its
  // own. Every one of them is a question with an answer, so each carries the question's id.
  it("understands the questions a plugin asks the core", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.save", id: "q1", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toEqual({
      type: "ft.save",
      id: "q1",
      name: "a.pdf",
      mime: "application/pdf",
      data: "AAA",
    });
    expect(said({ type: "ft.print", id: "q2", name: "a.pdf", mime: "application/pdf", data: "AAA" })).toMatchObject({
      type: "ft.print",
      id: "q2",
    });
    expect(said({ type: "ft.fetch", id: "q3", url: "https://api.openai.com/v1", method: "POST", headers: [], body: null })).toMatchObject({
      type: "ft.fetch",
      id: "q3",
      url: "https://api.openai.com/v1",
      method: "POST",
    });
    expect(said({ type: "ft.read", id: "q4", key: "pen" })).toEqual({ type: "ft.read", id: "q4", key: "pen" });
    expect(said({ type: "ft.write", id: "q5", key: "pen", value: "black" })).toEqual({
      type: "ft.write",
      id: "q5",
      key: "pen",
      value: "black",
    });
    expect(said({ type: "ft.forget", id: "q6", key: "pen" })).toEqual({ type: "ft.forget", id: "q6", key: "pen" });
    expect(said({ type: "ft.close" })).toEqual({ type: "ft.close" });

    // A question with no id could never be answered, and a fetch to nowhere is not a fetch.
    expect(said({ type: "ft.read", key: "pen" })).toBeNull();
    expect(said({ type: "ft.fetch", id: "q7" })).toBeNull();
    expect(said({ type: "ft.save", id: "q8", name: "a.pdf" })).toBeNull();

    // 2026-09-27: records, reminders, the live channel and the way back.
    expect(said({ type: "ft.recordSet", id: "r1", key: "note/1", value: "{}" })).toEqual({ type: "ft.recordSet", id: "r1", key: "note/1", value: "{}" });
    expect(said({ type: "ft.recordGet", id: "r2", key: "note/1" })).toEqual({ type: "ft.recordGet", id: "r2", key: "note/1" });
    expect(said({ type: "ft.recordKeys", id: "r3" })).toEqual({ type: "ft.recordKeys", id: "r3", prefix: "" });
    expect(said({ type: "ft.recordUsage", id: "r4" })).toEqual({ type: "ft.recordUsage", id: "r4" });
    expect(said({ type: "ft.remindSet", id: "r5", reminder: "n1", at: 1700, text: "milk" })).toEqual({ type: "ft.remindSet", id: "r5", reminder: "n1", at: 1700, text: "milk" });
    expect(said({ type: "ft.remindSet", id: "r6", reminder: "n1", at: "soon" })).toBeNull();
    expect(said({ type: "ft.remindCancel", id: "r7", reminder: "n1" })).toEqual({ type: "ft.remindCancel", id: "r7", reminder: "n1" });
    expect(said({ type: "ft.liveSend", id: "r8", data: "AQ==" })).toEqual({ type: "ft.liveSend", id: "r8", data: "AQ==" });
    expect(said({ type: "ft.liveSend", id: "r9", data: 7 })).toBeNull();
    expect(said({ type: "ft.openChat", id: "r10", ref: "ref_1" })).toEqual({ type: "ft.openChat", id: "r10", ref: "ref_1" });
    // The drive: an operation it knows and up to two strings; anything else is not a question.
    expect(said({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" })).toEqual({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" });
    expect(said({ type: "ft.drive", id: "r12", op: "status" })).toEqual({ type: "ft.drive", id: "r12", op: "status", a: "", b: "" });
    expect(said({ type: "ft.drive", id: "r13", op: "format" })).toBeNull();
    expect(said({ type: "ft.drive", op: "list" })).toBeNull();
    // 2026-10-02: where the phone is, once; a question with nothing but its id.
    expect(said({ type: "ft.location", id: "l1" })).toEqual({ type: "ft.location", id: "l1" });
    expect(said({ type: "ft.location" })).toBeNull();
  });

  // 2026-09-27: "open with": a plugin says which kinds of file it opens; a text goes only to
  // one that may read what it is handed.
  it("knows which plugins open a message", () => {
    expect(opensKind(["image/*"], "image/png")).toBe(true);
    expect(opensKind(["image/*"], "IMAGE/JPEG; charset=x")).toBe(true);
    expect(opensKind(["application/x-ftboard"], "application/x-ftboard")).toBe(true);
    expect(opensKind(["*/*"], "video/mp4")).toBe(true);
    expect(opensKind(["image/*"], "video/mp4")).toBe(false);
    expect(opensKind(undefined, "image/png")).toBe(false);

    const board = { ...CODE, id: "com.flickertalk.board", opens: ["application/x-ftboard", "image/*"] };
    const notes = { ...CODE, id: "com.flickertalk.notes", opens: ["text/plain"] };
    const deaf = { ...LOCKED, opens: ["text/plain"] };
    const drive = { ...LOCKED, id: "com.flickertalk.drive", opens: ["*/*"] };
    const ids = (list: { id: string }[]) => list.map((one) => one.id);
    expect(ids(openersOf([board, notes, deaf, drive], { text: "hi" }))).toEqual([notes.id]);
    expect(ids(openersOf([board, notes, deaf, drive], { kind: "file", file: { mime: "image/png" } }))).toEqual([board.id, drive.id]);
    expect(ids(openersOf([board, notes, deaf, drive], { kind: "file", file: { mime: "application/x-ftboard" } }))).toEqual([board.id, drive.id]);
    expect(ids(openersOf([board, notes], { kind: "file", file: {} }))).toEqual([]);
  });

  // 2026-09-27: a tap shows a file in its viewer; opening is not viewing.
  it("finds the viewer of a file, never a plugin that merely opens it, the latest first", () => {
    const drive = { ...CODE, id: "com.flickertalk.drive", opens: ["*/*"], installedAt: 9 };
    const board = { ...CODE, id: "com.flickertalk.board", opens: ["image/*", "application/pdf"], installedAt: 9 };
    const older = { ...CODE, id: "com.example.pdf-old", opens: ["application/pdf"], views: ["application/pdf"], installedAt: 1 };
    const newer = { ...CODE, id: "com.flickertalk.pdfviewer", opens: ["application/pdf"], views: ["application/pdf"], installedAt: 5 };
    expect(viewerOf([drive, board], "application/pdf")).toBeUndefined();
    expect(viewerOf([drive, board, older, newer], "application/pdf")?.id).toBe(newer.id);
    expect(viewerOf([drive, board, older, newer], "APPLICATION/PDF; charset=x")?.id).toBe(newer.id);
    expect(viewerOf([newer], "image/png")).toBeUndefined();
  });
});
