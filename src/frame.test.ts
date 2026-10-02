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
      observe() {}
    },
    () => 0,
  );
  const says = async (data: Said) => {
    window.dispatchEvent(new MessageEvent("message", { data }));
    await Promise.resolve();
  };
  return { posted, says, ran };
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

  // 2026-10-03 (Ioan): the app's colours, as Ionic's variables on the frame's root, there before
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
});
