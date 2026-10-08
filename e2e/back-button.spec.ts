// The back button on an iPhone (seen in the UI review, 2026-10-02): Ionic's iOS style writes a word
// beside the arrow, and it was its own English "Back" in every language. It is the app's word now.
// On Android (Material) the button is an arrow alone, as before.
import { devices } from "@playwright/test";
import { expect, test } from "./helpers";

test.describe("on an iPhone, in Arabic", () => {
  test.use({ userAgent: devices["iPhone 13"].userAgent, viewport: { width: 390, height: 844 }, locale: "ar" });

  // Settings → Blocked: a page pushed from Settings (Plugins is a tab since 2026-10-08).
  test("Settings → Blocked goes back with «رجوع», not «Back»", async ({ app }) => {
    await app.goto("/tabs/settings");
    await expect(app.locator("html")).toHaveClass(/\bios\b/);
    await app.getByTestId("blocked").click();
    const back = app.locator("ion-back-button:visible");
    await expect(back).toHaveCount(1);
    await expect(back.locator(".button-text")).toHaveText("رجوع");
  });
});

test.describe("on Android, in Arabic", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });

  test("the back button is an arrow alone", async ({ app }) => {
    await app.goto("/tabs/settings");
    await expect(app.locator("html")).toHaveClass(/\bmd\b/);
    await app.getByTestId("blocked").click();
    const back = app.locator("ion-back-button:visible");
    await expect(back).toHaveCount(1);
    await expect(back).toHaveClass(/back-button-has-icon-only/);
    await expect(back.locator(".button-text")).toHaveCount(0);
  });
});
