import { test as base, expect, type Page } from "@playwright/test";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { installFakeCore } from "./fake-core";

/** Every command the UI sent to the (fake) core so far, oldest first. */
export async function callsTo(page: Page): Promise<Array<[string, Record<string, unknown> | undefined]>> {
  return page.evaluate(() => (window as unknown as { __ftFake: { calls: Array<[string, Record<string, unknown> | undefined]> } }).__ftFake.calls);
}

export async function commandsSent(page: Page): Promise<string[]> {
  return (await callsTo(page)).map(([command]) => command);
}

/** The frame of the open plugin says something to the app, as its `frame.js` would. */
export async function frameSays(page: Page, message: Record<string, unknown>): Promise<void> {
  await page.evaluate((data) => {
    const frame = document.querySelector("iframe.ft-plugin__frame") as HTMLIFrameElement | null;
    if (!frame?.contentWindow) throw new Error("no plugin frame on the page");
    window.dispatchEvent(new MessageEvent("message", { data, source: frame.contentWindow }));
  }, message);
}

/**
 * Serves every plugin a frame of its own that only listens (2026-10-01): what the app tells it is
 * kept in `window.heard`, and `frameAsks` makes it ask, as a real plugin's `frame.js` would.
 */
export async function servePluginFrames(page: Page): Promise<void> {
  await page.route("http://ftplugin.localhost/**", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><script>
        window.heard = [];
        addEventListener("message", (event) => { if (event.source === parent) window.heard.push(event.data); });
        parent.postMessage({ type: "ft.ready" }, "*");
      </script>`,
    }),
  );
}

/** The open plugin asks the app something and gets its answer (`ft.done`), as a plugin would. */
export async function frameAsks(page: Page, message: Record<string, unknown>): Promise<unknown> {
  await expect.poll(() => page.frames().some((frame) => frame.url().startsWith("http://ftplugin.localhost/"))).toBe(true);
  const frame = page.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  await expect.poll(() => frame.evaluate(() => (window as unknown as { heard?: unknown[] }).heard !== undefined)).toBe(true);
  return frame.evaluate(async (asked) => {
    const heard = (window as unknown as { heard: Array<{ type: string; id?: string; answer?: unknown }> }).heard;
    parent.postMessage(asked, "*");
    for (let tries = 0; tries < 100; tries++) {
      const done = heard.find((one) => one.type === "ft.done" && one.id === asked.id);
      if (done) return done.answer;
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    throw new Error(`no answer to ${String(asked.type)}`);
  }, message);
}

/** Everything the open plugin's frame heard from the app. */
export async function frameHeard(page: Page): Promise<unknown[]> {
  const frame = page.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"));
  return frame ? frame.evaluate(() => (window as unknown as { heard: unknown[] }).heard) : [];
}

/**
 * Holds an app's tile, as a long press would (2026-10-08, plan of the apps grid): a right click,
 * which the tile takes as a hold. Its sheet opens; returns once its buttons are there.
 */
export async function holdTile(page: Page, testId: string): Promise<void> {
  await page.getByTestId(testId).click({ button: "right" });
  await expect(page.getByTestId("app-sheet")).toBeVisible();
}

/** A page with the fake core in place before the app loads. */
export const test = base.extend<{ app: Page }>({
  app: async ({ page }, use) => {
    await page.addInitScript(installFakeCore);
    await use(page);
  },
});

export { expect };

/**
 * Serves every plugin the frame the app really serves (2026-10-09): its page (`frame.html`), its
 * script, the Ionic it lends at `./ionic/` and the theme, under the policy of a plugin without
 * network (the frame's ancestor is the dev server here). `plugin` is the plugin's `dist/index.js`;
 * its element is `<ft-probe>`.
 */
export async function serveRealPluginFrames(page: Page, plugin: string): Promise<void> {
  const app = fileURLToPath(new URL("..", import.meta.url));
  const read = (path: string) => readFileSync(`${app}${path}`);
  const ionic = /pub const IONIC_VERSION: &str = "([^"]+)";/.exec(read("src-tauri/src/plugins.rs").toString())![1];
  const origins = "http://ftplugin.localhost https://ftplugin.localhost ftplugin://localhost";
  const policy =
    `default-src 'none'; script-src ${origins}; style-src ${origins} 'unsafe-inline'; img-src ${origins} data: blob:; ` +
    `font-src ${origins}; connect-src 'none'; base-uri 'none'; form-action 'none'; child-src 'none'; frame-ancestors http://localhost:1420`;
  const files: Record<string, () => [string, string | Buffer]> = {
    "frame.html": () => [
      "text/html; charset=utf-8",
      read("src-tauri/src/frame.html").toString().replaceAll("%IONIC%", ionic).replaceAll("%VERSION%", "1.0.0").replaceAll("%COMPONENT%", "ft-probe"),
    ],
    "frame.js": () => ["text/javascript", read("src-tauri/src/frame.js")],
    "ionic/ionic.js": () => ["text/javascript", read("src-tauri/resources/ionic/ionic.js")],
    "ionic/ionic.css": () => ["text/css", read("src-tauri/resources/ionic/ionic.css")],
    "ionic/theme.js": () => ["text/javascript", read("src-tauri/src/frame-theme.js")],
    "dist/index.js": () => ["text/javascript", plugin],
  };
  await page.route("http://ftplugin.localhost/**", (route) => {
    const file = new URL(route.request().url()).pathname.split("/").slice(2).join("/");
    const serve = files[file];
    if (!serve) return route.fulfill({ status: 404, body: "" });
    const [contentType, body] = serve();
    return route.fulfill({
      contentType,
      body,
      headers: { "Content-Security-Policy": policy, "Access-Control-Allow-Origin": "*", "Cache-Control": "no-store" },
    });
  });
}
