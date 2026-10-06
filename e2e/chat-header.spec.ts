// The chat header on a phone (2026-10-02, seen on a Samsung at 360 px): with the call, video and
// apps buttons (the games are a tab of the apps since Ioan's decision of 2026-10-02), a long name is
// cut with an ellipsis and never drawn under a button, in any language and either direction. The
// buttons keep their full tap size. Room for the name and status: about 98 px at 360 px and 150 px
// at 412 px (with four buttons it was 50 and 80).
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const LONG = "Mark (climbing) from the Tuesday bouldering group";

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) => a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test e2e/chat-header.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (dir) await app.screenshot({ path: `${dir}/${name}.png`, clip: { x: 0, y: 0, width: app.viewportSize()!.width, height: 120 } });
}

async function checkHeader(app: Page, name: string) {
  await app.addInitScript((long) => ((window as unknown as Record<string, unknown>).__ftFakeBobName = long), LONG);
  await app.goto(`/chat/${BOB}`);
  const bar = app.locator(".ft-thread__bar");
  const buttons = bar.locator("ion-buttons[slot='end'] ion-button");
  // Voice, video, the search (2026-10-05) and the apps (tools and games): four.
  await expect(buttons).toHaveCount(4);
  await expect(bar.locator(".ft-peer__name")).toHaveText(LONG);
  await app.waitForTimeout(300);
  await shot(app, name);

  const title = (await bar.locator(".ft-peer__text").boundingBox())!;
  for (const button of await buttons.all()) {
    const box = (await button.boundingBox())!;
    expect(box.width, "a full tap target").toBeGreaterThanOrEqual(44);
    expect(overlap(title, box), `the title ${JSON.stringify(title)} under a button ${JSON.stringify(box)}`).toBe(false);
  }
  // It is the name that gives way: cut with an ellipsis, not spilled under the buttons.
  const cut = await bar.locator(".ft-peer__name").evaluate((el) => ({
    truncates: el.scrollWidth > el.clientWidth,
    ellipsis: getComputedStyle(el).textOverflow,
  }));
  expect(cut).toEqual({ truncates: true, ellipsis: "ellipsis" });
}

test.describe("on a 360 px phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });
  test("a long name gives way to the four buttons", async ({ app }) => {
    await checkHeader(app, `phone-360-${process.env.FT_SHOTS_TAG ?? "now"}`);
  });
});

test("on a Pixel 7, a long name gives way to the four buttons", async ({ app }) => {
  await checkHeader(app, `pixel7-${process.env.FT_SHOTS_TAG ?? "now"}`);
});

test.describe("in Arabic, right to left", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });
  test("a long name gives way to the four buttons", async ({ app }) => {
    await checkHeader(app, `arabic-360-${process.env.FT_SHOTS_TAG ?? "now"}`);
    expect(await app.evaluate(() => document.documentElement.dir)).toBe("rtl");
    // A name keeps its own direction: a Latin name is cut at its end, not at its start.
    expect(await app.locator(".ft-peer__name").evaluate((el) => getComputedStyle(el).direction)).toBe("ltr");
  });
});
