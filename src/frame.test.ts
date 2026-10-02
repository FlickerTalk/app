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
  const code = source.replace('await import("./dist/index.js");', "await load();");
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
  delete root.dataset.dark;
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

  // 2026-10-02 (Ioan): the app's colours, as Ionic's variables on the frame's root, there before
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
});
