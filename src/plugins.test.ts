import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

import { fromFrame, frameUrl, readyPlugins } from "./plugins";

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

  // §53: a plugin only gets what the user hands it, and only if it was allowed to read it.
  it("offers only the plugins allowed to read what you send them", async () => {
    tauri.invoke.mockResolvedValue([CODE, LOCKED]);
    expect((await readyPlugins()).map((plugin) => plugin.id)).toEqual(["com.flickertalk.code"]);
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
});
