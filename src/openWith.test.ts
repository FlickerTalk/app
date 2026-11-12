import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { effectScope, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import { installTauri } from "./__tests__/tauri";
import { chat, type PluginView } from "./core";
import { premiumLocked, refreshPlugins } from "./plugins";
import { useOpenWith } from "./openWith";

const NOTES = {
  id: "com.flickertalk.notes",
  name: "Notes",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
  opens: ["text/plain"],
};
const VIEWER = {
  id: "com.flickertalk.pdfviewer",
  name: "PDF viewer",
  version: "1.0.0",
  asks: { network: [], messages: false, send: "nothing" },
  granted: { network: [], messages: false, send: "nothing" },
  installedAt: 2,
  opens: ["application/pdf"],
  views: ["application/pdf"],
};

async function withPlugins(plugins: object[], answers: Record<string, unknown> = {}) {
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_plugins") return plugins;
    if (command in answers) {
      const answer = answers[command];
      if (answer instanceof Error) throw answer;
      return answer;
    }
    return undefined;
  });
  await refreshPlugins();
  calls.length = 0;
}

function openWithIn(allowed: PluginView | undefined = undefined) {
  const acting = ref("");
  const closeActions = vi.fn(() => (acting.value = ""));
  const openPlugin = vi.fn();
  const allowedToRead = vi.fn(async () => allowed);
  const toPremium = vi.fn();
  const scope = effectScope();
  const open = scope.run(() =>
    useOpenWith({ messages: () => chat("c1")?.messages ?? [], acting, closeActions, openPlugin, allowedToRead, toPremium }),
  )!;
  return { open, acting, closeActions, openPlugin, allowedToRead, toPremium, stop: () => scope.stop() };
}

describe("useOpenWith", () => {
  beforeEach(() => seed());
  afterEach(() => (premiumLocked.value = false));

  it("lists the tools that open the message being acted on", async () => {
    await withPlugins([NOTES, VIEWER]);
    const { open, acting, stop } = openWithIn();
    expect(open.openers.value).toEqual([]);
    acting.value = "m1";
    expect(open.openers.value.map((one) => one.id)).toEqual([NOTES.id]);
    acting.value = "m4";
    expect(open.openers.value.map((one) => one.id)).toEqual([VIEWER.id]);
    expect(open.viewable.value).toBe(true);
    stop();
  });

  it("opens a text in the chosen tool with its reference", async () => {
    await withPlugins([NOTES], { core_plugin_ref: "ref_1" });
    const { open, acting, closeActions, openPlugin, stop } = openWithIn();
    acting.value = "m1";
    await open.openWith(NOTES.id);
    expect(closeActions).toHaveBeenCalled();
    expect(openPlugin).toHaveBeenCalledWith({
      id: NOTES.id,
      name: "Notes",
      sending: "nothing",
      live: false,
      text: "Are you coming tonight?",
      file: undefined,
      reference: "ref_1",
    });
    stop();
  });

  it("leads a locked tool to Premium, and opens nothing the tool may not read", async () => {
    await withPlugins([NOTES, { ...NOTES, id: "com.example.blind", granted: { ...NOTES.granted, messages: false } }]);
    const locked = openWithIn();
    premiumLocked.value = true;
    locked.acting.value = "m1";
    await locked.open.openWith(NOTES.id);
    expect(locked.toPremium).toHaveBeenCalled();
    expect(locked.openPlugin).not.toHaveBeenCalled();
    premiumLocked.value = false;
    locked.stop();

    const blind = openWithIn(undefined);
    blind.acting.value = "m1";
    await blind.open.openWith("com.example.blind");
    expect(blind.allowedToRead).toHaveBeenCalled();
    expect(blind.openPlugin).not.toHaveBeenCalled();
    blind.stop();
  });

  it("shows a tapped file in its viewer with the bytes, or hands it to another app", async () => {
    await withPlugins([VIEWER], { core_read_message_file: { name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" }, core_plugin_ref: "ref_4" });
    const viewer = openWithIn();
    await viewer.open.tapFile("m4");
    expect(viewer.openPlugin).toHaveBeenCalledWith(expect.objectContaining({ id: VIEWER.id, file: { name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" }, reference: "ref_4" }));
    expect(calls.some(([command]) => command === "core_open_file")).toBe(false);
    viewer.stop();

    await withPlugins([VIEWER], { core_read_message_file: new Error("that file is too big for a plugin") });
    const tooBig = openWithIn();
    await tooBig.open.tapFile("m4");
    expect(tooBig.openPlugin).not.toHaveBeenCalled();
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
    tooBig.stop();
  });

  it("hands a drive tool the file's name and type but not its bytes", async () => {
    await withPlugins([{ ...VIEWER, granted: { ...VIEWER.granted, drive: true } }]);
    const { open, openPlugin, stop } = openWithIn();
    await open.tapFile("m4");
    expect(openPlugin).toHaveBeenCalledWith(expect.objectContaining({ file: { name: "menu.pdf", mime: "application/pdf", data: "" } }));
    expect(calls.some(([command]) => command === "core_read_message_file")).toBe(false);
    stop();
  });

  it("opens the file in another app from the actions", async () => {
    await withPlugins([VIEWER]);
    const { open, acting, closeActions, stop } = openWithIn();
    acting.value = "m4";
    await open.openElsewhere();
    expect(closeActions).toHaveBeenCalled();
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
    stop();
  });
});
