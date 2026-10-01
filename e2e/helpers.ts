import { test as base, expect, type Page } from "@playwright/test";
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

/** A page with the fake core in place before the app loads. */
export const test = base.extend<{ app: Page }>({
  app: async ({ page }, use) => {
    await page.addInitScript(installFakeCore);
    await use(page);
  },
});

export { expect };
