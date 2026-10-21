// The frame bridge (`src-tauri/src/frame.js`, served as `frame.js` next to every plugin): what a
// plugin is handed and how. Run here as the frame runs it, with the plugin's own code, the parent
// window and the browser's observers standing in.
import { afterEach, describe, expect, it, vi } from "vitest";
import source from "../src-tauri/src/frame.js?raw";

type Said = Record<string, unknown>;

/** Runs the frame's script; `plugin` is what the plugin's `dist/index.js` does when it loads. */
async function frame(plugin: () => void = () => {}) {
  const posted: Said[] = [];
  const window = new EventTarget();
  const watched: { resized: () => void; targets: Element[] } = { resized: () => {}, targets: [] };
  // The plugin's code, at the version the frame was opened for (2026-10-03).
  const code = source.replace("await import(`./dist/index.js${new URL(import.meta.url).search}`);", "await load();");
  expect(code).not.toBe(source);
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const run = new AsyncFunction("parent", "load", "addEventListener", "removeEventListener", "ResizeObserver", "requestAnimationFrame", code);
  const ran = run(
    { postMessage: (message: Said) => posted.push(message) },
    async () => plugin(),
    window.addEventListener.bind(window),
    window.removeEventListener.bind(window),
    class {
      constructor(resized: () => void) {
        watched.resized = resized;
      }
      observe(target: Element) {
        watched.targets.push(target);
      }
    },
    () => 0,
  );
  const says = async (data: Said) => {
    window.dispatchEvent(new MessageEvent("message", { data }));
    await Promise.resolve();
  };
  return { posted, says, ran, watched };
}

const ft = () => (globalThis as unknown as { ft: { onOpen(handler: (opened: Said) => void): void } }).ft;

afterEach(() => {
  const root = document.documentElement;
  root.removeAttribute("style");
  root.removeAttribute("dir");
  delete root.dataset.dark;
  delete root.dataset.fill;
});

describe("the frame", () => {
  it("hands the plugin what the app opens it with, once the plugin is loaded", async () => {
    const opened: Said[] = [];
    const { posted, says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
    await says({ type: "ft.theme", dark: false, theme: {} });
    await ran;
    expect(posted).toContainEqual({ type: "ft.ready" });
    await says({ type: "ft.open", text: "hi", lang: "es", live: true, chat: "c".repeat(43) });
    expect(opened).toEqual([
      { text: "hi", dark: false, lang: "es", file: null, ref: null, reminder: null, live: true, theme: {}, chat: "c".repeat(43) },
    ]);
    expect(document.documentElement.lang).toBe("es");
  });

  // Plugin API 1.6.0: the app opens a plugin inside a call to lead or follow a presentation.
  it("tells a plugin opened in a call whether it leads or follows, and nothing else", async () => {
    const opened: Said[] = [];
    const { says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
    await says({ type: "ft.theme", dark: false, theme: {} });
    await ran;
    await says({ type: "ft.open", live: true, presenting: "follow" });
    await says({ type: "ft.open", live: true, presenting: "boss" });
    expect(opened[0]).toMatchObject({ live: true, presenting: "follow" });
    expect(opened[1]).not.toHaveProperty("presenting");
  });

  // 2026-10-02: the app's colours, as Ionic's variables on the frame's root, there before
  // the plugin draws anything and kept up to date; and whether the app is dark.
  const dark = {
    "--ion-background-color": "#000000",
    "--ion-text-color": "#f5f5f5",
    "--ion-color-medium": "#8e8e8e",
    "--ion-item-background": "#0e0e0e",
    "--ion-border-color": "rgba(255, 255, 255, 0.12)",
    "--ion-color-primary": "#ffffff",
    "--ion-color-primary-contrast": "#000000",
    "--ion-color-success": "#2dd55b",
    "--ion-color-danger": "hsl(353, 100%, 65%)",
  };
  const root = () => document.documentElement;
  const colour = (name: string) => root().style.getPropertyValue(name);

  it("asks for the app's colours and puts them on its root before the plugin loads", async () => {
    let seen = "";
    const { posted, says, ran } = await frame(() => {
      seen = colour("--ion-text-color");
    });
    expect(posted).toEqual([{ type: "ft.hello" }]);
    await says({ type: "ft.theme", dark: true, theme: dark });
    await ran;
    expect(seen).toBe("#f5f5f5");
    for (const [name, value] of Object.entries(dark)) expect(colour(name)).toBe(value);
    expect(root().dataset.dark).toBe("1");
    expect(root().style.colorScheme).toBe("dark");
  });

  it("takes only the nine colours, and only what looks like a colour", async () => {
    const opened: Said[] = [];
    const { says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
    const theme = {
      "--ion-text-color": "#f5f5f5",
      "--ion-color-primary": "url(https://evil.example/x)",
      "--ion-background-color": "red; background: url(x)",
      "--ion-color-danger": "rgb(255 77 94)",
      "--ft-secret": "#123456",
      color: "#123456",
    };
    await says({ type: "ft.theme", dark: false, theme });
    await ran;
    await says({ type: "ft.open", dark: false, theme });
    expect(colour("--ion-text-color")).toBe("#f5f5f5");
    expect(colour("--ion-color-danger")).toBe("rgb(255 77 94)");
    for (const name of ["--ion-color-primary", "--ion-background-color", "--ft-secret", "color"]) expect(colour(name)).toBe("");
    expect(opened[0].theme).toEqual({ "--ion-text-color": "#f5f5f5", "--ion-color-danger": "rgb(255 77 94)" });
    expect(root().dataset.dark).toBeUndefined();
    expect(root().style.colorScheme).toBe("light");
  });

  it("follows the app when its colours change while the plugin is open", async () => {
    const opened: Said[] = [];
    const { says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
    await says({ type: "ft.theme", dark: true, theme: dark });
    await ran;
    await says({ type: "ft.open", dark: true, theme: dark });
    expect(opened[0].dark).toBe(true);

    await says({ type: "ft.theme", dark: false, theme: { ...dark, "--ion-text-color": "#0a0a0a", "--ion-background-color": "#ffffff" } });
    expect(colour("--ion-text-color")).toBe("#0a0a0a");
    expect(colour("--ion-background-color")).toBe("#ffffff");
    expect(root().dataset.dark).toBeUndefined();
    expect(root().style.colorScheme).toBe("light");
  });

  // 2026-10-09: the app's text direction, on the frame's root, so Ionic's components (and the
  // overlays, which go to the body, outside the plugin's element) read right to left in Arabic.
  it("takes the app's direction and follows it", async () => {
    const { says, ran } = await frame();
    await says({ type: "ft.theme", dark: false, theme: {}, dir: "rtl" });
    await ran;
    expect(root().dir).toBe("rtl");
    await says({ type: "ft.theme", dark: false, theme: {}, dir: "ltr" });
    expect(root().dir).toBe("ltr");
    // Anything else is no direction: the last one stays.
    await says({ type: "ft.open", dark: false, theme: {}, dir: "sideways" });
    expect(root().dir).toBe("ltr");
  });

  // 2026-10-09: a tool's frame is as tall as its window; the page says so on its root, before the
  // plugin draws, so a plugin's element of `height: 100%` fills it.
  it("fills its window when the app says so", async () => {
    let filled: string | undefined;
    const { says, ran } = await frame(() => {
      filled = root().dataset.fill;
    });
    await says({ type: "ft.theme", dark: false, theme: {}, fill: true });
    await ran;
    expect(filled).toBe("1");
    await says({ type: "ft.theme", dark: false, theme: {} });
    expect(root().dataset.fill).toBeUndefined();
  });

  // 2026-10-09: which Ionic the app lends the frame, as the page says it (`data-ionic`).
  it("tells the plugin which Ionic it is lent", async () => {
    root().dataset.ionic = "9.0.4";
    try {
      const opened: Said[] = [];
      const { says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ type: "ft.open", text: "" });
      expect(opened[0].ionic).toEqual({ version: "9.0.4" });
    } finally {
      delete root().dataset.ionic;
    }
  });

  // 2026-10-20 (app#177): the phone's own translator, six questions to the app, each answered with
  // the contract's type whatever comes back (§4.2 of the translator's plan).
  it("asks the app to translate on the phone, and hands the plugin only the contract's answers", async () => {
    const { posted, says, ran } = await frame();
    await says({ type: "ft.theme", dark: false, theme: {} });
    await ran;
    type Translator = Record<string, (...args: unknown[]) => Promise<unknown>>;
    const translator = (globalThis as unknown as { ft: { translate: Translator } }).ft.translate;
    /** Calls `method`, checks what the frame asked the app, and answers it with `answer`. */
    const asked = async (method: string, args: unknown[], question: Said, answer: unknown) => {
      const pending = translator[method](...args);
      const last = posted[posted.length - 1];
      expect(last).toEqual({ ...question, id: expect.any(String) });
      await says({ type: "ft.done", id: last.id, answer });
      return pending;
    };
    expect(await asked("languages", [], { type: "ft.translateLanguages" }, ["en", "es"])).toEqual(["en", "es"]);
    expect(await asked("detect", ["hola"], { type: "ft.translateDetect", text: "hola" }, "es")).toBe("es");
    expect(await asked("status", ["es", "en"], { type: "ft.translateStatus", from: "es", to: "en" }, "downloadable")).toBe("downloadable");
    expect(await asked("prepare", ["es", "en"], { type: "ft.translatePrepare", from: "es", to: "en" }, true)).toBe(true);
    expect(await asked("remove", ["es", "en"], { type: "ft.translateRemove", from: "es", to: "en" }, true)).toBe(true);
    expect(await asked("translate", ["hola", "es", "en"], { type: "ft.translateText", text: "hola", from: "es", to: "en" }, "hello")).toBe("hello");

    // What is not the contract's answer is its empty answer: a refusal, a broken core, junk.
    expect(await asked("languages", [], { type: "ft.translateLanguages" }, null)).toEqual([]);
    expect(await asked("languages", [], { type: "ft.translateLanguages" }, ["en", 7, null])).toEqual(["en"]);
    expect(await asked("detect", [12], { type: "ft.translateDetect", text: "12" }, false)).toBeNull();
    expect(await asked("status", ["es", "xx"], { type: "ft.translateStatus", from: "es", to: "xx" }, "maybe")).toBe("unsupported");
    expect(await asked("prepare", ["es", "en"], { type: "ft.translatePrepare", from: "es", to: "en" }, "yes")).toBe(false);
    expect(await asked("remove", ["es", "en"], { type: "ft.translateRemove", from: "es", to: "en" }, null)).toBe(false);
    expect(await asked("translate", ["hola", "es", "en"], { type: "ft.translateText", text: "hola", from: "es", to: "en" }, false)).toBeNull();
  });

  it("loads the plugin all the same if the colours never come", async () => {
    vi.useFakeTimers();
    try {
      let loaded = false;
      const { ran } = await frame(() => {
        loaded = true;
      });
      await vi.advanceTimersByTimeAsync(2000);
      await ran;
      expect(loaded).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  // 2026-10-04: the frame follows its content down as well as up. The root's scrollHeight is never
  // less than the frame's own height, so a plugin that was once tall (a game's waiting screen)
  // kept its frame tall, with an empty scrollable area under it. What counts is the content: the
  // bottom of the body, margin included.
  it("tells the app how tall its content is, also when the content shrinks", async () => {
    const { posted, says, ran, watched } = await frame();
    await says({ type: "ft.theme", dark: false, theme: {} });
    await ran;
    expect(watched.targets).toContain(document.body);

    const body = document.body;
    const root = document.documentElement;
    let bottom = 900;
    vi.spyOn(body, "getBoundingClientRect").mockImplementation(() => ({ bottom }) as DOMRect);
    // The frame was made 900 px tall: the root scrolls that much, whatever the content is now.
    const scrolled = vi.spyOn(root, "scrollHeight", "get").mockReturnValue(900);
    body.style.marginBottom = "8px";
    try {
      watched.resized();
      expect(posted.at(-1)).toEqual({ type: "ft.height", height: 908 });
      bottom = 300;
      watched.resized();
      expect(posted.at(-1)).toEqual({ type: "ft.height", height: 308 });
    } finally {
      scrolled.mockRestore();
      vi.restoreAllMocks();
      body.style.marginBottom = "";
    }
  });

  // 2026-10-02: the app closes the window (its ✕, Android's Back, leaving the chat) with a word
  // first, so a plugin in a live session can say goodbye to its twin. The frame runs what the plugin
  // registered and answers when all of it is over, however it ended; the app waits only so long.
  describe("closing", () => {
    type Closing = { onClose(handler: () => unknown): void; close(): void };
    const closing = () => (globalThis as unknown as { ft: Closing }).ft;
    /** Only the promises already due: nothing here waits for a timer. */
    const settle = async () => {
      for (let turn = 0; turn < 10; turn++) await Promise.resolve();
    };
    const closedSaid = (posted: Said[]) => posted.filter((one) => one.type === "ft.closed").length;

    it("runs what the plugin registered and then says it is closed", async () => {
      const ran: string[] = [];
      const { posted, says, ran: loaded } = await frame(() => {
        closing().onClose(() => void ran.push("first"));
        closing().onClose(async () => {
          await Promise.resolve();
          ran.push("second");
        });
      });
      await says({ type: "ft.theme", dark: false, theme: {} });
      await loaded;
      expect(closedSaid(posted)).toBe(0);

      await says({ type: "ft.closing" });
      await settle();
      expect(ran).toEqual(["first", "second"]);
      expect(closedSaid(posted)).toBe(1);
      // Its goodbye went out before the answer, not after.
      expect(posted.at(-1)).toEqual({ type: "ft.closed" });
    });

    it("answers at once when the plugin registered nothing", async () => {
      const { posted, says, ran } = await frame();
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ type: "ft.closing" });
      await settle();
      expect(closedSaid(posted)).toBe(1);
    });

    it("answers once a slow goodbye is over, and only then", async () => {
      let finish = () => {};
      const { posted, says, ran } = await frame(() =>
        closing().onClose(() => new Promise<void>((resolve) => (finish = resolve))),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ type: "ft.closing" });
      await settle();
      expect(closedSaid(posted)).toBe(0);
      finish();
      await settle();
      expect(closedSaid(posted)).toBe(1);
    });

    it("answers all the same when a goodbye fails, and still runs the others", async () => {
      const ran: string[] = [];
      const { posted, says, ran: loaded } = await frame(() => {
        closing().onClose(() => {
          throw new Error("broken");
        });
        closing().onClose(() => Promise.reject(new Error("refused")));
        closing().onClose(() => void ran.push("third"));
      });
      await says({ type: "ft.theme", dark: false, theme: {} });
      await loaded;
      await says({ type: "ft.closing" });
      await settle();
      expect(ran).toEqual(["third"]);
      expect(closedSaid(posted)).toBe(1);
    });

    it("says goodbye once, however many times it is told", async () => {
      let times = 0;
      const { posted, says, ran } = await frame(() => closing().onClose(() => void (times += 1)));
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ type: "ft.closing" });
      await says({ type: "ft.closing" });
      await settle();
      expect(times).toBe(1);
      expect(closedSaid(posted)).toBeGreaterThanOrEqual(1);
    });

    // A goodbye that closes the window itself asks the app once more; the app is already closing.
    it("lets a goodbye ask for the window to close without running again", async () => {
      let times = 0;
      const { posted, says, ran } = await frame(() =>
        closing().onClose(() => {
          times += 1;
          closing().close();
        }),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ type: "ft.closing" });
      await settle();
      expect(times).toBe(1);
      expect(posted.filter((one) => one.type === "ft.close")).toHaveLength(1);
      expect(closedSaid(posted)).toBe(1);
    });
  });

  // 2026-10-20 (plan of the translator §8): the chat's hooks. The app hands the plugin a message,
  // a draft or the chat itself (`ft.hook`) and waits for its answer (`ft.hookDone`); a plugin running
  // out of sight may ask to be seen (`ft.show`).
  describe("hooks", () => {
    type Hooked = { onHook(handler: (hook: Said) => unknown): void; show(): void };
    const hooks = () => (globalThis as unknown as { ft: Hooked }).ft;
    const hook = { type: "ft.hook", id: "h1", name: "message", text: "see you at six", chat: "c".repeat(43), ref: "ref_1", lang: "es", manual: true };
    const done = (posted: Said[]) => posted.filter((one) => one.type === "ft.hookDone");

    it("hands the hook to the plugin's handler and its answer back to the app", async () => {
      const heard: Said[] = [];
      const { posted, says, ran } = await frame(() =>
        hooks().onHook(async (one) => {
          heard.push(one);
          return { text: "nos vemos a las seis" };
        }),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says(hook);
      await vi.waitFor(() => expect(done(posted)).toHaveLength(1));
      expect(heard).toEqual([{ name: "message", text: "see you at six", chat: "c".repeat(43), ref: "ref_1", lang: "es", manual: true }]);
      expect(done(posted)[0]).toEqual({ type: "ft.hookDone", id: "h1", result: { text: "nos vemos a las seis" } });
    });

    // 2026-10-21 (device QA of the translator): the run says what the tool runs on, so it asks
    // before a download only where the phone does not; anything but the three names is left out.
    it("tells the plugin's handler what it runs on, and nothing but android, ios or desktop", async () => {
      const heard: Said[] = [];
      const { posted, says, ran } = await frame(() =>
        hooks().onHook(async (one) => {
          heard.push(one);
          return null;
        }),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      for (const [id, platform] of [["a", "ios"], ["b", "android"], ["c", "desktop"], ["d", "windows"], ["e", 7]]) await says({ ...hook, id, platform });
      await vi.waitFor(() => expect(done(posted)).toHaveLength(5));
      expect(heard.map((one) => one.platform)).toEqual(["ios", "android", "desktop", undefined, undefined]);
      expect(heard[3]).not.toHaveProperty("platform");
    });

    // §8.2c: the chat's language as the plugin found it, and a question for the user (a download)
    // the app asks in its own alert before running the hook again with the answer.
    it("hands the settings hook's switches and language back, a question for the user, and only what a result can say", async () => {
      const answers: unknown[] = [
        { auto: { message: true, composer: false }, lang: "es" },
        { text: 7, auto: { message: "yes", composer: true }, lang: 5, secret: "x" },
        { ask: { text: "Download Spanish, about 30 MB?" } },
        { ask: "Download?" },
        "a text",
        {},
        null,
      ];
      const heard: Said[] = [];
      const { posted, says, ran } = await frame(() =>
        hooks().onHook(async (one) => {
          heard.push(one);
          return answers.shift();
        }),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      for (const id of ["a", "b", "c", "d", "e", "f", "g"]) await says({ ...hook, id, name: "settings", text: "", ref: null, manual: false, answer: id === "c" });
      await vi.waitFor(() => expect(done(posted)).toHaveLength(7));
      expect(done(posted).map((one) => one.result)).toEqual([
        { auto: { message: true, composer: false }, lang: "es" },
        { auto: { composer: true } },
        { ask: { text: "Download Spanish, about 30 MB?" } },
        null,
        null,
        null,
        null,
      ]);
      expect(heard[2]).toMatchObject({ answer: true });
      expect(heard[0]).not.toHaveProperty("answer");
    });

    // §8.2d: the settings hook. The app hands the values the user changed and takes back the
    // group as it is: values, the options of each select, the conversation's language (null: none).
    it("hands the settings hook the values the user changed, and takes back values, options and language", async () => {
      const heard: Said[] = [];
      const answers: unknown[] = [
        {
          values: { lang: "fr", receive: true, send: "yes", nested: { no: 1 } },
          options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }, { value: 7, label: "Seven" }], bad: "x" },
          lang: null,
          auto: { message: true, composer: false },
        },
      ];
      const { posted, says, ran } = await frame(() =>
        hooks().onHook(async (one) => {
          heard.push(one);
          return answers.shift() ?? null;
        }),
      );
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      await says({ ...hook, name: "settings", text: "", ref: null, values: { lang: "fr", receive: true, bad: { x: 1 } } });
      await vi.waitFor(() => expect(done(posted)).toHaveLength(1));
      expect(heard[0]).toMatchObject({ name: "settings", values: { lang: "fr", receive: true } });
      expect((heard[0].values as Record<string, unknown>).bad).toBeUndefined();
      expect(done(posted)[0].result).toEqual({
        values: { lang: "fr", receive: true, send: "yes" },
        options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French" }] },
        lang: null,
        auto: { message: true, composer: false },
      });
      await says({ ...hook, id: "h2" });
      await vi.waitFor(() => expect(done(posted)).toHaveLength(2));
      expect(heard[1]).not.toHaveProperty("values");
    });

    it("answers nothing for a plugin without a handler, a handler that fails or a hook it cannot know", async () => {
      const heard: Said[] = [];
      const bare = await frame();
      await bare.says({ type: "ft.theme", dark: false, theme: {} });
      await bare.ran;
      await bare.says(hook);
      await vi.waitFor(() => expect(done(bare.posted)).toEqual([{ type: "ft.hookDone", id: "h1", result: null }]));

      const broken = await frame(() =>
        hooks().onHook(async (one) => {
          heard.push(one);
          throw new Error("broken");
        }),
      );
      await broken.says({ type: "ft.theme", dark: false, theme: {} });
      await broken.ran;
      await broken.says(hook);
      await broken.says({ ...hook, id: "h2", name: "reaction" });
      await vi.waitFor(() => expect(done(broken.posted)).toHaveLength(2));
      expect(done(broken.posted).map((one) => one.result)).toEqual([null, null]);
      expect(heard).toHaveLength(1);
    });

    it("lets a plugin ask to be seen, and says which hook its window was opened for", async () => {
      const opened: Said[] = [];
      const { posted, says, ran } = await frame(() => ft().onOpen((one) => opened.push(one)));
      await says({ type: "ft.theme", dark: false, theme: {} });
      await ran;
      hooks().show();
      expect(posted).toContainEqual({ type: "ft.show" });
      await says({ type: "ft.open", hook: "settings", chat: "c".repeat(43) });
      await says({ type: "ft.open", hook: "chat" });
      expect(opened[0]).toMatchObject({ hook: "settings", chat: "c".repeat(43) });
      expect(opened[1]).not.toHaveProperty("hook");
    });
  });
});
