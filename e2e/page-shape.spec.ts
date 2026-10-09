// Ionic's own shape (2026-10-09): a page's header, content and footer are the page's own children,
// where Ionic's transitions look for them, with no frame of ours in between. The chat had it on
// its own page already; this checks the circle's page and the list of chats, where on a wide
// screen the pane with the conversation (or the circle) sits beside the list: only the pane has a
// frame, and it starts where the list ends, on the left in English and on the right in Arabic.
import { expect, test } from "./helpers";

/** The tags of the page's own children, overlays left out. */
const childrenOf = (page: import("@playwright/test").Locator) =>
  page.evaluate((element) =>
    Array.from(element.children)
      .map((child) => child.tagName.toLowerCase())
      .filter((tag) => !["ion-modal", "ion-alert", "ion-toast", "ion-loading", "ion-popover", "ion-action-sheet"].includes(tag)),
  );

test("the circle's header, content and footer are its page's own children", async ({ app }) => {
  await app.goto("/circle/circle1");
  await expect(app.getByTestId("circle-send")).toBeVisible();
  const page = app.locator("div.ion-page").filter({ has: app.getByTestId("circle-send") }).last();
  expect(await childrenOf(page)).toEqual(["ion-header", "ion-content", "ion-footer"]);
  // The composer sits at the bottom of the window, as in a conversation.
  const footer = (await page.locator(":scope > ion-footer").boundingBox())!;
  expect(Math.round(footer.y + footer.height)).toBe(app.viewportSize()!.height);
});

test("the list's header and content are the page's own children on a phone", async ({ app }) => {
  await app.goto("/tabs/chats");
  await expect(app.getByTestId("circle-row")).toBeVisible();
  expect(await childrenOf(app.locator(".ion-page.ft-chats"))).toEqual(["ion-header", "ion-content"]);
});

for (const locale of ["en", "ar"]) {
  test.describe(`tablet, ${locale}`, () => {
    test.use({ viewport: { width: 1280, height: 800 }, locale });

    test("the pane sits beside the list, which keeps to its width", async ({ app }) => {
      await app.goto("/tabs/chats");
      const page = app.locator(".ion-page.ft-chats");
      const pane = page.locator(":scope > .ft-chats__detail");
      await expect(pane.locator("[data-test='apps']")).toBeVisible();
      expect(await childrenOf(page)).toEqual(["ion-header", "ion-content", "section"]);

      const header = (await page.locator(":scope > ion-header").boundingBox())!;
      const list = (await page.locator(":scope > ion-content").boundingBox())!;
      const paneBox = (await pane.boundingBox())!;
      const pageBox = (await page.boundingBox())!;
      expect(Math.round(header.x)).toBe(Math.round(list.x));
      expect(Math.round(header.width)).toBe(Math.round(list.width));
      expect(Math.round(list.width)).toBeLessThan(Math.round(pageBox.width / 2));
      expect(Math.round(paneBox.height)).toBe(Math.round(pageBox.height));
      if (locale === "ar") {
        expect(Math.round(paneBox.x + paneBox.width)).toBe(Math.round(list.x));
        expect(Math.round(paneBox.x)).toBe(Math.round(pageBox.x));
      } else {
        expect(Math.round(paneBox.x)).toBe(Math.round(list.x + list.width));
        expect(Math.round(paneBox.x + paneBox.width)).toBe(Math.round(pageBox.x + pageBox.width));
      }

      // A circle beside the list: its header, content and footer in the pane's frame.
      await app.getByTestId("circle-row").click();
      await expect(pane.getByTestId("circle-send")).toBeVisible();
      expect(await childrenOf(pane.locator(":scope > .ft-thread"))).toEqual(["ion-header", "ion-content", "ion-footer"]);
      const footer = (await pane.locator(".ft-thread > ion-footer").boundingBox())!;
      expect(Math.round(footer.y + footer.height)).toBe(Math.round(paneBox.y + paneBox.height));
    });
  });
}
