// The on-screen keyboard (2026-09-29, seen on the iPhone 13 mini, the Lenovo tablet and the
// Samsung S20+): the WebViews do not shrink the page for it, they only shrink the visual viewport
// and pan it to show the focused field, and the chat's header went off the screen. Here the
// keyboard is played by a visual viewport the test moves, as the WebViews move theirs: the page
// keeps its height, what can be seen shrinks, and may start lower (the pan).
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

/** Before the app loads: a visual viewport that follows the real one until a keyboard opens. */
function installKeyboard() {
  const real = window.visualViewport!;
  let keyboard: { height: number; pan: boolean } | null = null;
  const fake = new EventTarget();
  Object.defineProperties(fake, {
    height: { get: () => (keyboard ? window.innerHeight - keyboard.height : real.height) },
    offsetTop: { get: () => (keyboard?.pan ? keyboard.height : 0) },
    width: { get: () => real.width },
    offsetLeft: { get: () => 0 },
    pageTop: { get: () => (keyboard?.pan ? keyboard.height : 0) },
    pageLeft: { get: () => 0 },
    scale: { get: () => 1 },
  });
  real.addEventListener("resize", () => fake.dispatchEvent(new Event("resize")));
  Object.defineProperty(window, "visualViewport", { value: fake, configurable: true });
  (window as unknown as { __keyboard: (height: number, pan: boolean) => void }).__keyboard = (height, pan) => {
    keyboard = height > 0 ? { height, pan } : null;
    fake.dispatchEvent(new Event("resize"));
  };
  // A conversation longer than the screen.
  (window as unknown as { __ftFakeLongChat: number }).__ftFakeLongChat = 40;
}

async function keyboard(page: Page, height: number, pan = false) {
  await page.evaluate(([h, p]) => (window as unknown as { __keyboard: (h: number, p: boolean) => void }).__keyboard(h, p), [height, pan] as const);
}

/** Top and bottom on the page, in CSS pixels. */
async function edges(locator: Locator) {
  const box = await locator.boundingBox();
  if (!box) throw new Error("not on screen");
  return { top: Math.round(box.y), bottom: Math.round(box.y + box.height) };
}

/** What can be seen of the page: from the viewport's top, as tall as it is. */
async function visible(page: Page) {
  return page.evaluate(() => {
    const viewport = window.visualViewport!;
    return { top: viewport.offsetTop, bottom: viewport.offsetTop + viewport.height };
  });
}

test.beforeEach(async ({ app }) => {
  await app.addInitScript(installKeyboard);
});

async function checkThread(page: Page, header: Locator, composer: Locator, last: Locator) {
  await expect(last).toBeInViewport();
  const before = await edges(header);
  const composerBefore = await edges(composer);

  for (const pan of [false, true]) {
    await composer.click();
    await keyboard(page, 330, pan);
    const seen = await visible(page);
    // The header stays at the top of what can be seen; the composer sits right on the keyboard.
    await expect.poll(async () => (await edges(header)).top).toBe(before.top + seen.top);
    expect((await edges(header)).bottom - (await edges(header)).top).toBe(before.bottom - before.top);
    const footer = await edges(composer);
    expect(footer.bottom).toBeLessThanOrEqual(seen.bottom);
    expect(footer.bottom).toBeGreaterThan(seen.bottom - 30);
    // The conversation that showed its last message still does, above the composer.
    const message = await edges(last);
    expect(message.bottom).toBeLessThanOrEqual(footer.top);
    expect(message.top).toBeGreaterThanOrEqual((await edges(header)).bottom);

    // Closing the keyboard puts everything back.
    await keyboard(page, 0, false);
    await expect.poll(async () => edges(header)).toEqual(before);
    await expect.poll(async () => (await edges(composer)).bottom).toBe(composerBefore.bottom);
  }
}

test("a chat keeps its header when the keyboard opens, and the last message above the composer", async ({ app }) => {
  await app.goto("/chat/ft_bob123456789");
  const thread = app.locator(".ft-thread");
  await checkThread(
    app,
    thread.locator("ion-header"),
    thread.locator("ion-footer"),
    thread.getByTestId("bubble").last(),
  );
});

test("a circle keeps its header when the keyboard opens", async ({ app }) => {
  await app.goto("/circle/circle1");
  const thread = app.locator(".ft-thread");
  await checkThread(app, thread.locator("ion-header"), thread.locator("ion-footer"), thread.getByTestId("bubble").last());
});

test.describe("on a wide screen (the tablet)", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the list and the chat beside it keep their headers", async ({ app }) => {
    await app.goto("/tabs/chats");
    const thread = app.locator(".ft-chats__detail .ft-thread");
    const listHeader = app.locator(".ft-chats ion-header").first();
    await expect(thread.getByTestId("bubble").last()).toBeInViewport();
    const listBefore = await edges(listHeader);
    await checkThread(app, thread.locator("ion-header"), thread.locator("ion-footer"), thread.getByTestId("bubble").last());

    await thread.locator("ion-footer").click();
    await keyboard(app, 360, true);
    const seen = await visible(app);
    await expect.poll(async () => (await edges(listHeader)).top).toBe(listBefore.top + seen.top);
  });
});
