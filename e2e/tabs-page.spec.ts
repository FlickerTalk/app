import { expect, test } from "./helpers";

const IPHONE =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

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
      await expect(app.locator("ion-tab-bar")).toBeHidden();
    });
  });
}

test.describe("iPhone", () => {
  test.use({ userAgent: IPHONE });

  test("the tabs slide away while a page opens over them", async ({ app }) => {
    await app.goto("/tabs/settings");
    await expect(app.getByTestId("backup")).toBeVisible();
    await app.evaluate(() => {
      const tabs = document.querySelector("ion-tabs")!;
      const seen: number[] = [];
      (window as unknown as { __ftTabsSeen: number[] }).__ftTabsSeen = seen;
      const sample = () => {
        seen.push(tabs.getBoundingClientRect().x);
        if (seen.length < 120) requestAnimationFrame(sample);
      };
      requestAnimationFrame(sample);
    });
    await app.getByTestId("backup").click();
    await expect(app.locator("ion-title", { hasText: "Backup" }).last()).toBeVisible();
    await app.waitForTimeout(800);
    const seen = await app.evaluate(() => (window as unknown as { __ftTabsSeen: number[] }).__ftTabsSeen);
    expect(Math.min(...seen), "the tabs moved to the side").toBeLessThan(-20);
  });
});
