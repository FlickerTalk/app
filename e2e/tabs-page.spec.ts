// The tabs page in Ionic's own shape (2026-10-09): `ion-page > ion-tabs`, the outlet and the bottom
// bar inside. Ionic's page transitions look for the tabs among the page's own children; with a frame
// of ours in between, the iPhone left the tabs still while the next page's title slid over them. On
// a wide screen the rail sits beside the tabs, and the tabs start where the rail ends: on the left in
// English, on the right in Arabic.
import { expect, test } from "./helpers";

test.describe("phone", () => {
  test("the tabs are the page's own child, and fill it", async ({ app }) => {
    await app.goto("/tabs/chats");
    const tabs = app.locator("ion-tabs");
    await expect(tabs).toBeVisible();
    expect(await tabs.evaluate((element) => element.parentElement?.classList.contains("ion-page"))).toBe(true);
    const box = (await tabs.boundingBox())!;
    const viewport = app.viewportSize()!;
    expect(Math.round(box.x)).toBe(0);
    expect(Math.round(box.width)).toBe(viewport.width);
    await expect(app.locator("ion-tab-bar")).toBeVisible();
  });
});

for (const locale of ["en", "ar"]) {
  test.describe(`tablet, ${locale}`, () => {
    test.use({ viewport: { width: 1280, height: 800 }, locale });

    test("the tabs start where the rail ends and reach the other edge", async ({ app }) => {
      await app.goto("/tabs/settings");
      const tabs = app.locator("ion-tabs");
      await expect(tabs).toBeVisible();
      expect(await tabs.evaluate((element) => element.parentElement?.classList.contains("ion-page"))).toBe(true);
      const rail = (await app.locator("nav.ft-rail").boundingBox())!;
      const box = (await tabs.boundingBox())!;
      if (locale === "ar") {
        expect(Math.round(box.x)).toBe(0);
        expect(Math.round(box.x + box.width)).toBe(Math.round(rail.x));
      } else {
        expect(Math.round(box.x)).toBe(Math.round(rail.x + rail.width));
        expect(Math.round(box.x + box.width)).toBe(1280);
      }
      // The bottom bar gives way to the rail.
      await expect(app.locator("ion-tab-bar")).toBeHidden();
    });
  });
}
