import type { Locator, Page } from "@playwright/test";
import { expect, frameHeard, servePluginFrames, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

const SIZES = [
  { name: "phone", width: 360, height: 740 },
  { name: "tablet", width: 1280, height: 800 },
];

async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(400);
  await app.screenshot({ path: `${dir}/${name}.png` });
}

async function playFromTheChat(app: Page, wide: boolean) {
  const apps: Locator = wide ? app.locator(".ft-chats__detail [data-test='apps']") : app.getByTestId("apps");
  await app.goto(wide ? "/tabs/chats" : `/chat/${BOB}`);
  await apps.click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId(`game-${TICTACTOE}`).click();
  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId("game-room")).toBeVisible();
  await expect.poll(async () => (await frameHeard(app)).length).toBeGreaterThan(0);
}

async function markFrame(app: Page) {
  await app.locator("iframe.ft-plugin__frame").evaluate((frame) => ((frame as unknown as { marked: boolean }).marked = true));
  const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  await frame.evaluate(() => ((window as unknown as { marked: boolean }).marked = true));
}

async function stillMarked(app: Page): Promise<[boolean, boolean]> {
  const element = await app.locator("iframe.ft-plugin__frame").evaluate((frame) => (frame as unknown as { marked?: boolean }).marked === true);
  const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  const document = await frame.evaluate(() => (window as unknown as { marked?: boolean }).marked === true);
  return [element, document];
}

async function lastFill(app: Page): Promise<unknown> {
  const heard = (await frameHeard(app)) as Array<{ type?: string; fill?: boolean }>;
  return heard.filter((one) => one.type === "ft.theme").at(-1)?.fill;
}

for (const size of SIZES) {
  test.describe(size.name, () => {
    test.use({ viewport: { width: size.width, height: size.height } });
    test.beforeEach(async ({ app }) => servePluginFrames(app));
    const wide = size.width >= 768;

    test("a game in a chat covers the whole window with the same frame and gives the room back", async ({ app }) => {
      await playFromTheChat(app, wide);
      const room = (await app.getByTestId("game-area").boundingBox())!;
      await markFrame(app);

      await app.getByTestId("game-fullscreen").click();
      await expect(app.getByTestId("game-fullscreen-bar")).toBeVisible();
      await expect.poll(async () => app.getByTestId("game-area").boundingBox()).toEqual({ x: 0, y: 0, width: size.width, height: size.height });
      for (const [x, y] of [
        [10, size.height / 2],
        [size.width / 4, size.height - 10],
        [size.width - 10, 10],
      ]) {
        const inside = await app.evaluate(([px, py]) => Boolean(document.elementFromPoint(px, py)?.closest("[data-test='game-area']")), [x, y]);
        expect(inside, `${x},${y}`).toBe(true);
      }
      await expect.poll(() => lastFill(app)).toBe(true);
      await expect(app.getByTestId("game-fullscreen-exit")).toBeFocused();
      expect(await stillMarked(app)).toEqual([true, true]);
      await shot(app, `fullscreen-room-${size.name}`);

      await app.getByTestId("game-fullscreen-exit").click();
      await expect(app.getByTestId("game-fullscreen-bar")).toHaveCount(0);
      await expect.poll(async () => app.getByTestId("game-area").boundingBox()).toEqual(room);
      await expect.poll(() => lastFill(app)).toBe(false);
      await expect(app.getByTestId("game-fullscreen")).toBeFocused();
      expect(await stillMarked(app)).toEqual([true, true]);
    });

    test("Back ends full screen first, and Close from full screen leaves nothing over the chat", async ({ app }) => {
      await playFromTheChat(app, wide);
      await app.getByTestId("game-fullscreen").click();
      await expect(app.getByTestId("game-fullscreen-bar")).toBeVisible();
      expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(true);
      await expect(app.getByTestId("game-fullscreen-bar")).toHaveCount(0);
      await expect(app.getByTestId("game-room")).toBeVisible();

      await app.getByTestId("game-fullscreen").click();
      await app.getByTestId("game-fullscreen-close").click();
      await expect(app.getByTestId("game-room")).toHaveCount(0);
      expect(await app.evaluate(() => document.querySelectorAll(".ft-room__game--fullscreen").length)).toBe(0);
      await expect(app.getByRole("button", { name: "Video call", exact: true })).toBeVisible();
    });
  });
}

test.describe("a game on its own page", () => {
  test.use({ viewport: { width: 1280, height: 800 } });
  test.beforeEach(async ({ app }) => servePluginFrames(app));

  test("swaps the page's bar for the full-screen one and lets the frame reach the sides", async ({ app }) => {
    await app.goto(`/plugin/${TICTACTOE}`);
    const frame = app.locator("iframe.ft-plugin__frame");
    await expect(frame).toBeVisible();
    const before = (await frame.boundingBox())!;

    await app.getByTestId("game-fullscreen").click();
    await expect(app.getByTestId("game-fullscreen-bar")).toBeVisible();
    await expect(app.locator("ion-back-button")).toHaveCount(0);
    await expect.poll(async () => (await frame.boundingBox())?.width).toBe(1280);
    const after = (await frame.boundingBox())!;
    expect(after.height).toBeGreaterThanOrEqual(before.height);
    await shot(app, "fullscreen-page-tablet");

    expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(true);
    await expect(app.getByTestId("game-fullscreen-bar")).toHaveCount(0);
    await expect.poll(async () => frame.boundingBox()).toEqual(before);
  });
});
