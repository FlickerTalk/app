// The frame bridge (`src-tauri/src/frame.js`, served as `frame.js` next to every plugin): what a
// plugin is handed and how. Run here as the frame runs it, with the plugin's own code, the parent
// window and the browser's observers standing in.
import { afterEach, describe, expect, it } from "vitest";
import source from "../src-tauri/src/frame.js?raw";

type Said = Record<string, unknown>;

/** Runs the frame's script; `plugin` is what the plugin's `dist/index.js` does when it loads. */
async function frame(plugin: () => void = () => {}) {
  const posted: Said[] = [];
  const window = new EventTarget();
  const code = source.replace('await import("./dist/index.js");', "await load();");
  expect(code).not.toBe(source);
  const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;
  const run = new AsyncFunction("parent", "load", "addEventListener", "ResizeObserver", "requestAnimationFrame", code);
  const ran = run(
    { postMessage: (message: Said) => posted.push(message) },
    async () => plugin(),
    window.addEventListener.bind(window),
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
    await ran;
    expect(posted).toContainEqual({ type: "ft.ready" });
    await says({ type: "ft.open", text: "hi", lang: "es", live: true, chat: "c".repeat(43) });
    expect(opened).toEqual([
      { text: "hi", dark: false, lang: "es", file: null, ref: null, reminder: null, live: true, chat: "c".repeat(43) },
    ]);
    expect(document.documentElement.lang).toBe("es");
  });
});
