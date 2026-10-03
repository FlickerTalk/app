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
