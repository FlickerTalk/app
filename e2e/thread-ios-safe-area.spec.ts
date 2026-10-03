// The conversation on an iPhone with a home indicator (seen on the iPhone 13 mini, 2026-10-03):
// the phone reports the indicator's band in `--ion-safe-area-bottom` (34 px there), and what sits
// at the bottom of the thread keeps clear of it, with the band at the very bottom of the screen.
import { devices, type Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const INSET = 34;

test.use({ userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 } });

test.beforeEach(async ({ app }) => {
  // As the phone reports it. Set once the page exists: an init script runs before <html> does.
  await app.addInitScript((inset) => {
    document.addEventListener("DOMContentLoaded", () => document.documentElement.style.setProperty("--ion-safe-area-bottom", `${inset}px`));
  }, INSET);
});

/** Top and bottom of what the selector finds, in CSS pixels. */
async function edges(app: Page, selector: string) {
  const box = await app.locator(selector).boundingBox();
  if (!box) throw new Error(`${selector} is not on screen`);
  return { top: Math.round(box.y), bottom: Math.round(box.y + box.height) };
}

test("the emoji panel sits right under the message box, and the home indicator's band under the panel (app#72)", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await expect(app.locator("html")).toHaveClass(/\bios\b/);
  const height = app.viewportSize()!.height;
  // Closed, the message box keeps clear of the indicator.
  expect((await edges(app, ".ft-composer__row")).bottom).toBeLessThanOrEqual(height - INSET);

  await app.getByTestId("open-emoji").click();
  await expect(app.locator(".ft-emoji")).toBeVisible();
  const row = await edges(app, ".ft-composer__row");
  const panel = await edges(app, ".ft-emoji");
  // No empty band between the message box and the panel: only the composer's own spacing.
  expect(panel.top - row.bottom, `a gap of ${panel.top - row.bottom} px above the panel`).toBeLessThanOrEqual(8);
  // The panel's last row of emoji stands above the indicator; the panel's colour runs to the edge.
  expect((await edges(app, ".ft-emoji__grid")).bottom).toBeLessThanOrEqual(height - INSET);
  expect(panel.bottom).toBe(height);
});

/** How far the thread is from its end: 0 when its last pixel shows. */
async function gapToEnd(app: Page) {
  return app.locator(".ft-thread ion-content").evaluate(async (content) => {
    const scroller = await (content as HTMLIonContentElement).getScrollElement();
    return Math.round(scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop);
  });
}

test.describe("a conversation longer than the screen", () => {
  test.beforeEach(async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeLongChat = 40));
  });

  // On the iPhone the thread stopped 60 px short: the footer, its home indicator's band or the last
  // message took their final height only after the scroll to the end. Played here by growing them
  // once the thread is at its end.
  async function openAtEnd(app: Page) {
    await app.goto("/tabs/chats");
    await app.getByTestId("chat-row").filter({ hasText: "Bob" }).first().click();
    await expect(app.locator(".ft-thread").getByTestId("bubble").last()).toBeVisible();
    await expect.poll(() => gapToEnd(app)).toBeLessThanOrEqual(1);
  }

  test("opens at its end, the last message above the message box (app#81)", async ({ app }) => {
    await openAtEnd(app);
    const bubble = (await app.locator(".ft-thread").getByTestId("bubble").last().boundingBox())!;
    const footer = (await app.locator(".ft-thread ion-footer").boundingBox())!;
    expect(bubble.y + bubble.height).toBeLessThanOrEqual(footer.y);
  });

  test("stays at its end when the footer grows after it opened (app#81)", async ({ app }) => {
    await openAtEnd(app);
    await app.evaluate(() => document.documentElement.style.setProperty("--ion-safe-area-bottom", "94px"));
    await expect.poll(() => gapToEnd(app)).toBeLessThanOrEqual(1);
  });

  test("stays at its end when its last message grows after it opened, as a card or a picture does (app#81)", async ({ app }) => {
    await openAtEnd(app);
    await app.locator(".ft-thread").getByTestId("bubble").last().evaluate((bubble) => ((bubble as HTMLElement).style.minHeight = "200px"));
    await expect.poll(() => gapToEnd(app)).toBeLessThanOrEqual(1);
  });

  test("scrolled back on purpose, is not taken to its end when the footer grows (app#81)", async ({ app }) => {
    await openAtEnd(app);
    await app.locator(".ft-thread ion-content").evaluate(async (content) => ((await (content as HTMLIonContentElement).getScrollElement()).scrollTop = 200));
    await expect.poll(() => gapToEnd(app)).toBeGreaterThan(1000);
    await app.evaluate(() => document.documentElement.style.setProperty("--ion-safe-area-bottom", "94px"));
    await app.waitForTimeout(300);
    expect(await gapToEnd(app)).toBeGreaterThan(1000);
  });
});
