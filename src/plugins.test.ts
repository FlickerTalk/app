import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

import { fromFrame, frameUrl, installedPlugins } from "./plugins";

const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
};
const LOCKED = { ...CODE, id: "com.example.locked", granted: { network: [], messages: false, send: "nothing" } };

describe("plugins in the app", () => {
  beforeEach(() => tauri.invoke.mockReset());

  // The apps of this phone are all the ones installed: a tool that works on a picture never
  // needed to read a message (§53). What was granted decides what it is handed, not whether it is
  // there.
  it("offers every app that is installed", async () => {
    tauri.invoke.mockResolvedValue([CODE, LOCKED]);
    expect((await installedPlugins()).map((plugin) => plugin.id)).toEqual([CODE.id, LOCKED.id]);
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
  });
});
