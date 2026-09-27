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

/** A page with the fake core in place before the app loads. */
export const test = base.extend<{ app: Page }>({
  app: async ({ page }, use) => {
    await page.addInitScript(installFakeCore);
    await use(page);
  },
});

export { expect };
