import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
const events = vi.hoisted(() => ({ handlers: new Map<string, () => void>(), unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: () => void) => {
    events.handlers.set(name, handler);
    return events.unlisten;
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

import {
  byPluginName,
  followPluginChanges,
  fromFrame,
  NOTICE_LIMIT,
  frameUrl,
  games,
  hookIcon,
  hookLabel,
  hookPlatform,
  hooked,
  settingsFields,
  settingsNote,
  installed,
  isLocked,
  followPremiumLock,
  offered,
  offeredGames,
  offeredOnce,
  openersOf,
  opensKind,
  PLUGIN_ICONS,
  pluginIcon,
  pluginImage,
  pluginName,
  pluginSummary,
  refreshOffered,
  refreshPlugins,
  refreshPremiumLock,
  tools,
  premiumLocked,
  viewerOf,
} from "./plugins";
import type { OfferedPlugin, PluginView } from "./core";
import { setLocale } from "./i18n";
import { bookOutline, extensionPuzzleOutline, languageOutline, gameControllerOutline, imageOutline, logoMarkdown, radioButtonOnOutline } from "ionicons/icons";
import { readdirSync } from "node:fs";

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

  it("offers every app that is installed", async () => {
    tauri.invoke.mockResolvedValue([CODE, LOCKED]);
    expect((await refreshPlugins()).map((plugin) => plugin.id)).toEqual([CODE.id, LOCKED.id]);
  });

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

  it("reads the plugins again when the core says they changed", async () => {
    tauri.invoke.mockResolvedValue([{ ...CODE, version: "1.0.1" }]);
    const stop = await followPluginChanges();
    events.handlers.get("ft://plugins")?.();
    await vi.waitFor(() => expect(installed.value.map((one) => one.version)).toEqual(["1.0.1"]));
    stop();
    expect(events.unlisten).toHaveBeenCalled();
    installed.value = [];
  });

  it("reads the plugins once as it starts following them", async () => {
    tauri.invoke.mockResolvedValue([CODE]);
    const stop = await followPluginChanges();
    await vi.waitFor(() => expect(installed.value.map((one) => one.id)).toEqual([CODE.id]));
    expect(tauri.invoke.mock.calls.filter(([command]) => command === "core_plugins")).toHaveLength(1);
    stop();
    installed.value = [];
  });

  it("reads the plugins for whoever waits on them only if nobody read them yet", async () => {
    vi.resetModules();
    const fresh = await import("./plugins");
    tauri.invoke.mockResolvedValue([CODE]);
    expect((await fresh.pluginsReady()).map((one) => one.id)).toEqual([CODE.id]);
    expect(tauri.invoke.mock.calls.filter(([command]) => command === "core_plugins")).toHaveLength(1);
    await fresh.pluginsReady();
    expect(tauri.invoke.mock.calls.filter(([command]) => command === "core_plugins")).toHaveLength(1);
  });

  it("shares the first read of the plugins while it is on its way", async () => {
    vi.resetModules();
    const fresh = await import("./plugins");
    tauri.invoke.mockResolvedValue([CODE]);
    const first = fresh.refreshPlugins();
    await fresh.pluginsReady();
    await first;
    expect(tauri.invoke.mock.calls.filter(([command]) => command === "core_plugins")).toHaveLength(1);
  });

  it("names the installed version in the frame's address", () => {
    installed.value = [{ ...CODE, version: "1.0.1" }];
    expect(frameUrl("com.flickertalk.code")).toBe("http://ftplugin.localhost/com.flickertalk.code/frame.html?v=1.0.1");
    installed.value = [];
  });

  it("listens only to the frame of the plugin", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const good = { source: frame.contentWindow, data: { type: "ft.height", height: 120 } } as unknown as MessageEvent;
    expect(fromFrame(good, frame)).toEqual({ type: "ft.height", height: 120 });

    const elsewhere = { source: {}, data: { type: "ft.height", height: 1 } } as unknown as MessageEvent;
    expect(fromFrame(elsewhere, frame)).toBeNull();

    const nonsense = { source: frame.contentWindow, data: { type: "ft.doSomething" } } as unknown as MessageEvent;
    expect(fromFrame(nonsense, frame)).toBeNull();
  });

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

  it("understands a plugin asking for a photo from the camera", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.takePhoto", id: "q1" })).toEqual({ type: "ft.takePhoto", id: "q1" });
    expect(said({ type: "ft.takePhoto" })).toBeNull();
    expect(said({ type: "ft.takePhoto", id: 7 })).toBeNull();
  });

  it("understands a plugin's notice", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) =>
      fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);

    expect(said({ type: "ft.notify", text: "Your turn" })).toEqual({ type: "ft.notify", text: "Your turn", sticky: false });
    expect(said({ type: "ft.notify", text: "Waiting…", sticky: true })).toEqual({ type: "ft.notify", text: "Waiting…", sticky: true });
    expect(said({ type: "ft.notify", text: "Hi", sticky: "yes" })).toEqual({ type: "ft.notify", text: "Hi", sticky: false });
    expect(said({ type: "ft.notify", text: "" })).toEqual({ type: "ft.notify", text: "", sticky: false });
    expect(said({ type: "ft.notify", text: "  Your\nturn\u0007 \u009b" })).toEqual({ type: "ft.notify", text: "Your turn", sticky: false });
    expect(said({ type: "ft.notify", text: "\u0000\u001b" })).toEqual({ type: "ft.notify", text: "", sticky: false });

    const long = said({ type: "ft.notify", text: "😀".repeat(500) });
    expect(long && "text" in long && Array.from(long.text)).toHaveLength(NOTICE_LIMIT);
    expect(long && "text" in long && long.text.endsWith("…")).toBe(true);

    expect(said({ type: "ft.notify" })).toBeNull();
    expect(said({ type: "ft.notify", text: 7 })).toBeNull();
  });

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
    expect(said({ type: "ft.closed" })).toEqual({ type: "ft.closed" });

    expect(said({ type: "ft.read", key: "pen" })).toBeNull();
    expect(said({ type: "ft.fetch", id: "q7" })).toBeNull();
    expect(said({ type: "ft.save", id: "q8", name: "a.pdf" })).toBeNull();

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
    expect(said({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" })).toEqual({ type: "ft.drive", id: "r11", op: "mkdir", a: "Docs", b: "" });
    expect(said({ type: "ft.drive", id: "r12", op: "status" })).toEqual({ type: "ft.drive", id: "r12", op: "status", a: "", b: "" });
    expect(said({ type: "ft.drive", id: "r13", op: "format" })).toBeNull();
    expect(said({ type: "ft.drive", op: "list" })).toBeNull();
    expect(said({ type: "ft.location", id: "l1" })).toEqual({ type: "ft.location", id: "l1" });
    expect(said({ type: "ft.location" })).toBeNull();
    expect(said({ type: "ft.translateLanguages", id: "t1" })).toEqual({ type: "ft.translateLanguages", id: "t1" });
    expect(said({ type: "ft.translateLanguages" })).toBeNull();
    expect(said({ type: "ft.translateDetect", id: "t2", text: "hola" })).toEqual({ type: "ft.translateDetect", id: "t2", text: "hola" });
    expect(said({ type: "ft.translateDetect", id: "t2" })).toBeNull();
    for (const type of ["ft.translateStatus", "ft.translatePrepare", "ft.translateRemove"]) {
      expect(said({ type, id: "t3", from: "es", to: "en", extra: 1 })).toEqual({ type, id: "t3", from: "es", to: "en" });
      expect(said({ type, id: "t3", from: "es" })).toBeNull();
      expect(said({ type, id: "t3", from: 7, to: "en" })).toBeNull();
      expect(said({ type, from: "es", to: "en" })).toBeNull();
    }
    expect(said({ type: "ft.translateText", id: "t4", text: "hola", from: "es", to: "en" })).toEqual({ type: "ft.translateText", id: "t4", text: "hola", from: "es", to: "en" });
    expect(said({ type: "ft.translateText", id: "t4", text: ["hola"], from: "es", to: "en" })).toBeNull();
    expect(said({ type: "ft.translateText", id: "t4", text: "hola", from: "es" })).toBeNull();
  });

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
    const blind = { ...CODE, id: "com.example.blind", opens: ["text/plain"], asks: { ...CODE.asks, messages: false }, granted: { ...CODE.granted, messages: false } };
    const drive = { ...LOCKED, id: "com.flickertalk.drive", opens: ["*/*"], asks: { ...LOCKED.asks, messages: false } };
    const ids = (list: { id: string }[]) => list.map((one) => one.id);
    expect(ids(openersOf([board, notes, deaf, blind, drive], { text: "hi" }))).toEqual([notes.id, deaf.id]);
    expect(ids(openersOf([board, notes, deaf, drive], { kind: "file", file: { mime: "image/png" } }))).toEqual([board.id, drive.id]);
    expect(ids(openersOf([board, notes, deaf, drive], { kind: "file", file: { mime: "application/x-ftboard" } }))).toEqual([board.id, drive.id]);
    expect(ids(openersOf([board, notes], { kind: "file", file: {} }))).toEqual([]);
  });

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

  it("keeps the tools and the games of this phone apart", async () => {
    const chess: PluginView = { ...CODE, id: "com.flickertalk.game.chess", name: "Chess", kind: "game" };
    const marked: PluginView = { ...CODE, id: "com.flickertalk.notes", kind: "tool" };
    const odd = { ...CODE, id: "com.example.odd", kind: "toy" } as unknown as PluginView;
    tauri.invoke.mockResolvedValue([CODE, chess, marked, odd]);
    await refreshPlugins();
    expect(tools.value.map((plugin) => plugin.id)).toEqual([CODE.id, marked.id, odd.id]);
    expect(games.value.map((plugin) => plugin.id)).toEqual([chess.id]);
  });

  it("never opens a message or shows a file with a game", () => {
    const game: PluginView = {
      ...CODE,
      id: "com.flickertalk.game.chess",
      kind: "game",
      opens: ["*/*", "text/plain"],
      views: ["application/pdf"],
      installedAt: 99,
    };
    expect(openersOf([game], { text: "hi" })).toEqual([]);
    expect(openersOf([game], { kind: "file", file: { mime: "image/png" } })).toEqual([]);
    expect(viewerOf([game], "application/pdf")).toBeUndefined();
  });

  describe("what the catalogue offers", () => {
    const CHESS: OfferedPlugin = {
      id: "com.flickertalk.game.chess",
      name: "Chess",
      version: "1.0.0",
      summary: "Play chess.",
      size: 120_000,
      installed: false,
      carried: false,
      kind: "game",
    };
    const SKETCH: OfferedPlugin = { ...CHESS, id: "com.flickertalk.sketch", name: "Sketch", kind: undefined, carried: true };

    beforeEach(() => {
      offered.value = [];
    });

    it("keeps what the catalogue offers where every screen reads it", async () => {
      tauri.invoke.mockResolvedValue([SKETCH, CHESS]);
      await refreshOffered();
      expect(tauri.invoke).toHaveBeenCalledWith("core_catalogue");
      expect(offered.value.map((one) => one.id)).toEqual([SKETCH.id, CHESS.id]);
      expect(offeredGames.value.map((one) => one.id)).toEqual([CHESS.id]);
    });

    it("asks the catalogue once at a time", async () => {
      tauri.invoke.mockResolvedValue([SKETCH, CHESS]);
      await Promise.all([offeredOnce(), offeredOnce()]);
      await offeredOnce();
      expect(tauri.invoke).toHaveBeenCalledTimes(1);
      expect(offeredGames.value.map((one) => one.id)).toEqual([CHESS.id]);
    });

    it("asks the catalogue even when the games the app carries are already known", async () => {
      const CARRIED_CHESS = { ...CHESS, carried: true };
      const GO = { ...CHESS, id: "com.flickertalk.game.go", name: "Go" };
      offered.value = [SKETCH, CARRIED_CHESS];
      tauri.invoke.mockResolvedValue([SKETCH, CARRIED_CHESS, GO]);
      await offeredOnce();
      expect(tauri.invoke).toHaveBeenCalledTimes(1);
      expect(offeredGames.value.map((one) => one.id)).toEqual([CHESS.id, GO.id]);
    });

    it("does not ask again after it has asked once", async () => {
      tauri.invoke.mockResolvedValue([SKETCH]);
      await offeredOnce();
      tauri.invoke.mockResolvedValue([SKETCH, CHESS]);
      await offeredOnce();
      expect(tauri.invoke).toHaveBeenCalledTimes(1);
      await refreshOffered();
      await offeredOnce();
      expect(tauri.invoke).toHaveBeenCalledTimes(3);
    });

    it("keeps what it had when the core cannot answer", async () => {
      tauri.invoke.mockResolvedValue([SKETCH, CHESS]);
      await refreshOffered();
      tauri.invoke.mockRejectedValueOnce(new Error("no core"));
      await expect(refreshOffered()).rejects.toThrow("no core");
      expect(offeredGames.value.map((one) => one.id)).toEqual([CHESS.id]);
    });
  });

  it("understands a hook's answer and a plugin asking to be seen, and nothing more", () => {
    const frame = { contentWindow: {} } as unknown as HTMLIFrameElement;
    const said = (data: unknown) => fromFrame({ source: frame.contentWindow, data } as unknown as MessageEvent, frame);
    expect(said({ type: "ft.show" })).toEqual({ type: "ft.show" });
    expect(said({ type: "ft.hookDone", id: "h1", result: { text: "hola" } })).toEqual({ type: "ft.hookDone", id: "h1", result: { text: "hola" } });
    expect(said({ type: "ft.hookDone", id: "h2", result: { auto: { message: true, composer: "yes" }, secret: 1 } })).toEqual({
      type: "ft.hookDone",
      id: "h2",
      result: { auto: { message: true } },
    });
    expect(said({ type: "ft.hookDone", id: "h3", result: null })).toEqual({ type: "ft.hookDone", id: "h3", result: null });
    expect(said({ type: "ft.hookDone", id: "h4", result: "hola" })).toEqual({ type: "ft.hookDone", id: "h4", result: null });
    expect(said({ type: "ft.hookDone", id: "h5", result: { text: "x".repeat(5_001) } })).toEqual({ type: "ft.hookDone", id: "h5", result: null });
    expect(said({ type: "ft.hookDone", result: { text: "hola" } })).toBeNull();
    expect(said({ type: "ft.hookDone", id: "h6", result: { lang: "es", auto: { message: true } } })).toEqual({ type: "ft.hookDone", id: "h6", result: { lang: "es", auto: { message: true } } });
    expect(said({ type: "ft.hookDone", id: "h7", result: { lang: "x".repeat(36) } })).toEqual({ type: "ft.hookDone", id: "h7", result: null });
    expect(said({ type: "ft.hookDone", id: "h10", result: { lang: "" } }), "no language any more").toEqual({ type: "ft.hookDone", id: "h10", result: { lang: "" } });
    expect(said({ type: "ft.hookDone", id: "h11", result: { lang: null } }), "none, as the SDK says it").toEqual({ type: "ft.hookDone", id: "h11", result: { lang: null } });
    const group = {
      values: { lang: "fr", receive: true, send: 3, deep: { x: 1 } },
      options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }, { value: "de" }], other: "x" },
      lang: "fr",
    };
    expect(said({ type: "ft.hookDone", id: "h12", result: group })).toEqual({
      type: "ft.hookDone",
      id: "h12",
      result: { values: { lang: "fr", receive: true }, options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }] }, lang: "fr" },
    });
    const many = { options: { lang: Array.from({ length: 400 }, (_, at) => ({ value: `l${at}`, label: `Language ${at}` })) } };
    expect((said({ type: "ft.hookDone", id: "h13", result: many }) as unknown as { result: { options: { lang: unknown[] } } }).result.options.lang, "no list longer than the languages there are").toHaveLength(300);
    expect(said({ type: "ft.hookDone", id: "h8", result: { ask: { text: "Download Spanish,\nabout 30 MB?" } } })).toEqual({
      type: "ft.hookDone",
      id: "h8",
      result: { ask: { text: "Download Spanish, about 30 MB?" } },
    });
    expect(said({ type: "ft.hookDone", id: "h9", result: { ask: { text: "  " } } })).toEqual({ type: "ft.hookDone", id: "h9", result: null });
  });

  describe("the chat's hooks", () => {
    const SETTINGS = {
      label: "Translation",
      icon: "language-outline",
      fields: [
        { key: "lang", type: "select" as const, label: "Language" },
        { key: "receive", type: "toggle" as const, label: "Translate what I receive" },
      ],
      note: "Language packs are downloaded once.",
    };
    const TRANSLATOR: PluginView = {
      ...CODE,
      id: "com.flickertalk.translate",
      name: "Translator",
      kind: "tool",
      hooks: {
        message: { label: "Translate", icon: "language-outline" },
        composer: { label: "Translate", icon: "not-an-icon-we-carry" },
        settings: SETTINGS,
      },
      locales: {
        es: { name: "Traductor", hooks: { message: "Traducir", composer: "  ", settings: { label: "Traducción", fields: { lang: "Idioma", receive: " " }, note: "Los paquetes se descargan una vez." } } },
      },
    };
    const CHESS: PluginView = { ...CODE, id: "com.flickertalk.game.chess", name: "Chess", kind: "game", hooks: { settings: SETTINGS } };
    const NOTES: PluginView = { ...CODE, id: "com.flickertalk.notes", name: "Notes", hooks: { message: { label: "Keep", icon: "book-outline" } } };

    beforeEach(async () => {
      installed.value = [];
      await setLocale("en");
    });

    it("lists the tools bound to a hook, by name, never a game", () => {
      installed.value = [TRANSLATOR, CHESS, NOTES, CODE];
      expect(hooked("message").map((one) => one.id)).toEqual(["com.flickertalk.notes", "com.flickertalk.translate"]);
      expect(hooked("settings").map((one) => one.id)).toEqual(["com.flickertalk.translate"]);
      expect(hooked("composer").map((one) => one.id)).toEqual(["com.flickertalk.translate"]);
    });

    it("names a hook in the phone's language, else in English", async () => {
      expect(hookLabel(TRANSLATOR, "message")).toBe("Translate");
      expect(hookLabel(TRANSLATOR, "settings")).toBe("Translation");
      await setLocale("es");
      expect(hookLabel(TRANSLATOR, "message")).toBe("Traducir");
      expect(hookLabel(TRANSLATOR, "settings"), "the settings' own label").toBe("Traducción");
      expect(hookLabel(TRANSLATOR, "composer"), "an empty label is no label").toBe("Translate");
      expect(hookLabel(CODE, "message"), "no hook, no label").toBe("");
    });

    it("names the settings' fields and note in the phone's language, else in English", async () => {
      expect(settingsFields(TRANSLATOR)).toEqual([
        { key: "lang", type: "select", label: "Language" },
        { key: "receive", type: "toggle", label: "Translate what I receive" },
      ]);
      expect(settingsNote(TRANSLATOR)).toBe("Language packs are downloaded once.");
      await setLocale("es");
      expect(settingsFields(TRANSLATOR).map((field) => field.label)).toEqual(["Idioma", "Translate what I receive"]);
      expect(settingsNote(TRANSLATOR)).toBe("Los paquetes se descargan una vez.");
      expect(settingsFields(NOTES), "no settings, no fields").toEqual([]);
      expect(settingsNote(NOTES)).toBe("");
    });

    it("tells a hook what it runs on: an iPhone or an iPad, an Android phone, else a desktop", () => {
      const on = (...platforms: string[]) => (name: string) => platforms.includes(name);
      expect(hookPlatform(on("ios", "iphone", "mobile"))).toBe("ios");
      expect(hookPlatform(on("ios", "ipad", "tablet")), "an iPad too").toBe("ios");
      expect(hookPlatform(on("android", "mobile"))).toBe("android");
      expect(hookPlatform(on("android", "tablet")), "an Android tablet too").toBe("android");
      expect(hookPlatform(on("desktop"))).toBe("desktop");
      expect(hookPlatform(on()), "anything else").toBe("desktop");
    });

    it("draws a hook with the icon it names if the app carries it, else the tool's own", () => {
      expect(hookIcon(TRANSLATOR, "message")).toBe(languageOutline);
      expect(hookIcon(TRANSLATOR, "composer")).toBe(extensionPuzzleOutline);
      expect(hookIcon(CODE, "settings")).toBe(extensionPuzzleOutline);
    });
  });

  describe("names and summaries in the phone's language", () => {
    const LIST: PluginView = {
      ...CODE,
      id: "com.flickertalk.list",
      name: "List",
      kind: "tool",
      locales: { es: { name: "Listas" }, zh: { name: "清单" } },
    };
    const LISTED: OfferedPlugin = {
      id: "com.flickertalk.list",
      name: "List",
      version: "1.0.1",
      summary: "A list you both edit.",
      size: 9000,
      installed: true,
      carried: false,
      locales: {
        es: { name: "Lista vieja", summary: "Una lista que editáis los dos." },
        "zh-TW": { name: "清單", summary: "兩人一起編輯的清單。" },
      },
    };

    beforeEach(async () => {
      installed.value = [];
      offered.value = [];
      await setLocale("en");
    });

    it("is the English one on an English phone, and without any translation", async () => {
      installed.value = [LIST];
      offered.value = [LISTED];
      expect(pluginName(LIST)).toBe("List");
      expect(pluginSummary(LISTED)).toBe("A list you both edit.");
      await setLocale("fr");
      expect(pluginName(LIST)).toBe("List");
      expect(pluginSummary(LISTED)).toBe("A list you both edit.");
    });

    it("is the installed package's own, before the catalogue's", async () => {
      installed.value = [LIST];
      offered.value = [LISTED];
      await setLocale("es");
      expect(pluginName(LIST)).toBe("Listas");
      expect(pluginName(LISTED), "the same plugin, wherever it is shown").toBe("Listas");
      expect(pluginSummary(LISTED)).toBe("Una lista que editáis los dos.");
    });

    it("is the catalogue's when the installed package has none", async () => {
      const bare = { ...LIST, locales: undefined };
      installed.value = [bare];
      offered.value = [LISTED];
      await setLocale("es");
      expect(pluginName(bare)).toBe("Lista vieja");
      expect(pluginName({ id: LIST.id, name: "List" })).toBe("Lista vieja");
    });

    it("is the exact language first, then the base language of a regional one", async () => {
      installed.value = [LIST];
      offered.value = [LISTED];
      await setLocale("zh-TW");
      expect(pluginName(LIST), "the catalogue's zh-TW before the package's zh").toBe("清單");
      await setLocale("zh-CN");
      expect(pluginName(LIST), "no zh-CN anywhere: the base language").toBe("清单");
      expect(pluginSummary(LISTED), "zh-TW is not zh-CN").toBe("A list you both edit.");
    });

    it("never takes an empty translation", async () => {
      const blank = { ...LIST, locales: { es: { name: "  ", summary: "" } } };
      await setLocale("es");
      expect(pluginName(blank)).toBe("List");
    });

    it("sorts by the name the phone shows, in the phone's own order", async () => {
      const named = (id: string, name: string, es: string): PluginView => ({ ...CODE, id, name, locales: { es: { name: es } } });
      const list = [named("a", "Zebra", "Árbol"), named("b", "Apple", "Zorro"), named("c", "Mango", "Bingo")];
      await setLocale("es");
      expect(byPluginName(list).map(pluginName)).toEqual(["Árbol", "Bingo", "Zorro"]);
      expect(list.map((one) => one.id), "a sorted copy").toEqual(["a", "b", "c"]);
      installed.value = list;
      expect(tools.value.map(pluginName)).toEqual(["Árbol", "Bingo", "Zorro"]);
      await setLocale("en");
      expect(tools.value.map(pluginName)).toEqual(["Apple", "Mango", "Zebra"]);
    });
  });

  describe("the tools' lock", () => {
    const GAME = { ...CODE, id: "game.flickertalk.chess", kind: "game" as const };

    it("locks the tools, never the games, only once the plan is limited", async () => {
      for (const [state, locked] of [["trial", false], ["subscribed", false], ["limited", true]] as const) {
        tauri.invoke.mockReset().mockResolvedValue({ state, until: 0 });
        expect(await refreshPremiumLock(), state).toBe(locked);
        expect(premiumLocked.value, state).toBe(locked);
        expect(isLocked(CODE), state).toBe(locked);
        expect(isLocked(GAME), state).toBe(false);
      }
      expect(tauri.invoke.mock.calls[0][0]).toBe("core_plan");
    });

    it("locks nothing when the plan cannot be read", async () => {
      tauri.invoke.mockReset().mockRejectedValueOnce(new Error("no core"));
      expect(await refreshPremiumLock()).toBe(false);
      expect(isLocked(CODE)).toBe(false);
    });

    it("reads the plan again whenever the core says it changed", async () => {
      tauri.invoke.mockReset().mockResolvedValue({ state: "trial", until: 0 });
      await followPremiumLock();
      tauri.invoke.mockResolvedValue({ state: "limited", until: 0 });
      events.handlers.get("ft://plan")?.();
      await vi.waitFor(() => expect(premiumLocked.value).toBe(true));
      premiumLocked.value = false;
    });
  });
  describe("the icon of a tile", () => {
    it("draws the Ionicon the plugin names", () => {
      expect(pluginIcon({ icon: "image-outline" })).toBe(imageOutline);
      expect(pluginIcon({ icon: "logo-markdown", kind: "tool" })).toBe(logoMarkdown);
      expect(pluginIcon({ icon: "radio-button-on-outline", kind: "game" })).toBe(radioButtonOnOutline);
      expect(pluginIcon({ icon: "book-outline" })).toMatch(/^data:image\/svg\+xml/);
      expect(pluginIcon({ icon: "book-outline" })).toBe(bookOutline);
    });

    it("draws its own for a name it does not allow, or none", () => {
      expect(pluginIcon({ icon: "rocket-outline-of-nowhere" })).toBe(extensionPuzzleOutline);
      expect(pluginIcon({ icon: "toString" })).toBe(extensionPuzzleOutline);
      expect(pluginIcon({ icon: "__proto__", kind: "game" })).toBe(gameControllerOutline);
      expect(pluginIcon({})).toBe(extensionPuzzleOutline);
      expect(pluginIcon({ kind: "tool", icon: "" })).toBe(extensionPuzzleOutline);
      expect(pluginIcon({ kind: "game" })).toBe(gameControllerOutline);
      expect(pluginIcon({ kind: "game", icon: "nope" })).toBe(gameControllerOutline);
    });

    it("allows every icon our plugins use and every icon of the frame", () => {
      const chosen = [
        "image-outline", "logo-markdown", "document-outline", "eye-off-outline", "brush-outline", "scan-outline",
        "timer-outline", "reader-outline", "easel-outline", "cloud-outline", "book-outline", "sparkles-outline",
        "list-outline", "location-outline", "stats-chart-outline", "pencil-outline", "cut-outline", "grid-outline",
        "ellipse-outline", "shield-outline", "radio-button-on-outline", "dice-outline", "square-outline",
        "albums-outline", "apps-outline", "basket-outline", "contrast-outline", "boat-outline", "text-outline",
        "language-outline",
      ];
      const frame = readdirSync("src-tauri/resources/icons").filter((file) => file.endsWith(".svg")).map((file) => file.slice(0, -4));
      expect(frame.length).toBeGreaterThan(40);
      for (const name of [...chosen, ...frame]) {
        expect(Object.keys(PLUGIN_ICONS), name).toContain(name);
        expect(PLUGIN_ICONS[name], name).toMatch(/^data:image\/svg\+xml/);
      }
    });
  });

  describe("the image of a tile", () => {
    it("is a base64 data URL of the SVG, UTF-8 included, or nothing", () => {
      const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64"><title>Échecs ♞</title><rect width="64" height="64"/></svg>';
      const url = pluginImage({ image: svg })!;
      expect(url.startsWith("data:image/svg+xml;base64,")).toBe(true);
      const bytes = Uint8Array.from(atob(url.slice("data:image/svg+xml;base64,".length)), (c) => c.charCodeAt(0));
      expect(new TextDecoder().decode(bytes)).toBe(svg);
      expect(pluginImage({})).toBeUndefined();
      expect(pluginImage({ image: "" })).toBeUndefined();
    });
  });
});
