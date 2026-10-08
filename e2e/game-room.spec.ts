// The game room (Ioan, 2026-10-02, option A): a game is played inside the conversation, so the
// two can write while they play. The header (with the voice call) stays on top, the composer at the
// bottom, a line above it shows what the other one writes, and the thread is one tap away. The game
// keeps running while the thread is shown, and across a trip to the call screen.
import type { Locator, Page } from "@playwright/test";
import { callsTo, expect, frameHeard, frameSays, servePluginFrames, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

type Box = { x: number; y: number; width: number; height: number };
const overlap = (a: Box, b: Box) => a.x < b.x + b.width - 0.5 && b.x < a.x + a.width - 0.5 && a.y < b.y + b.height - 0.5 && b.y < a.y + a.height - 0.5;

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test e2e/game-room.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(400);
  await app.screenshot({ path: `${dir}/${name}.png` });
}

async function box(locator: Locator): Promise<Box> {
  await expect(locator).toBeVisible();
  return (await locator.boundingBox())!;
}

/** The other phone writes in this conversation, as the core would announce it. */
async function bobWrites(app: Page, text: string) {
  await app.evaluate((said) => {
    const fake = (window as unknown as { __ftFake: { state: { messages: Record<string, unknown[]> }; emit: (event: string, payload: unknown) => void } }).__ftFake;
    fake.state.messages["ft_bob123456789"].push({ id: `bob-${said}`, outgoing: false, text: said, sentAt: Date.now(), state: "delivered" });
    fake.emit("ft://changed", { contact: "ft_bob123456789" });
  }, text);
}

/** Tic-tac-toe, from the apps sheet of the conversation on screen, granted on the way. */
async function playFromTheChat(app: Page, apps: Locator) {
  await apps.click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId(`game-${TICTACTOE}`).click();
  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId("game-room")).toBeVisible();
}

/** Every part of the room on the screen, one under the other, none over another. */
async function laidOut(app: Page, size: { width: number; height: number }) {
  const parts = {
    header: await box(app.locator(".ft-thread__bar").last()),
    bar: await box(app.getByTestId("game-bar")),
    game: await box(app.getByTestId("game-area")),
    strip: await box(app.getByTestId("game-strip")),
    composer: await box(app.locator(".ft-composer")),
  };
  for (const [name, one] of Object.entries(parts)) {
    expect(one.x, name).toBeGreaterThanOrEqual(-0.5);
    expect(one.y, name).toBeGreaterThanOrEqual(-0.5);
    expect(one.x + one.width, name).toBeLessThanOrEqual(size.width + 0.5);
    expect(one.y + one.height, name).toBeLessThanOrEqual(size.height + 0.5);
  }
  const order = [parts.header, parts.bar, parts.game, parts.strip, parts.composer];
  for (let at = 1; at < order.length; at++) expect(overlap(order[at - 1], order[at]), `part ${at} over part ${at - 1}`).toBe(false);
  // The messages wait underneath.
  await expect(app.locator(".ft-thread__content").last()).toBeHidden();
}

const SIZES = [
  { name: "phone", width: 360, height: 740 },
  { name: "tablet", width: 1280, height: 800 },
];

for (const appearance of ["dark", "light"]) {
  for (const size of SIZES) {
    test.describe(`${size.name}, ${appearance}`, () => {
      test.use({ viewport: { width: size.width, height: size.height } });
      test.beforeEach(async ({ app }) => {
        await app.addInitScript((chosen) => localStorage.setItem("ft-appearance", chosen), appearance);
        await servePluginFrames(app);
      });

      test("the game sits between the header and the composer; writing and reading go on", async ({ app }) => {
        if (size.width < 768) {
          await app.goto(`/chat/${BOB}`);
          await playFromTheChat(app, app.getByTestId("apps"));
        } else {
          // The tablet: the conversation beside the list, and the game in it.
          await app.goto("/tabs/chats");
          await playFromTheChat(app, app.locator(".ft-chats__detail [data-test='apps']"));
          await expect(app.locator(".ft-chats__list")).toBeVisible();
        }
        await laidOut(app, size);
        // No video and no files while playing.
        await expect(app.getByRole("button", { name: "Video call", exact: true })).toHaveCount(0);
        await expect(app.getByRole("button", { name: "Attach", exact: true })).toHaveCount(0);
        await expect(app.getByRole("button", { name: "Voice call", exact: true })).toBeVisible();

        await bobWrites(app, "your move!");
        await expect(app.getByTestId("game-strip")).toContainText("your move!");
        await shot(app, `${size.name}-room-${appearance}`);

        // Writing back without leaving the game.
        await app.locator("ion-textarea textarea").fill("thinking…");
        await app.getByRole("button", { name: "Send", exact: true }).click();
        await expect.poll(async () => (await callsTo(app)).some(([command, args]) => command === "core_send" && args?.text === "thinking…")).toBe(true);
        await expect(app.getByTestId("game-room")).toBeVisible();

        // The thread on a tap, and back to the game.
        await app.getByTestId("game-peek").click();
        await expect(app.locator(".ft-thread__content").last()).toBeVisible();
        await expect(app.getByTestId("game-area")).toBeHidden();
        await shot(app, `${size.name}-peek-${appearance}`);
        await app.getByTestId("game-peek").click();
        await expect(app.getByTestId("game-area")).toBeVisible();
      });
    });
  }
}

test.describe("on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });
  test.beforeEach(async ({ app }) => servePluginFrames(app));

  // The keyboard shrinks what can be seen (viewport.ts): the composer sits on it, the game area
  // gets smaller and scrolls, and nothing goes under anything.
  test("with the keyboard open the composer stays above it and the game gives way", async ({ app }) => {
    await app.addInitScript(() => {
      const real = window.visualViewport!;
      let keyboard = 0;
      const fake = new EventTarget();
      Object.defineProperties(fake, {
        height: { get: () => window.innerHeight - keyboard },
        offsetTop: { get: () => 0 },
        width: { get: () => real.width },
        offsetLeft: { get: () => 0 },
        pageTop: { get: () => 0 },
        pageLeft: { get: () => 0 },
        scale: { get: () => 1 },
      });
      Object.defineProperty(window, "visualViewport", { value: fake, configurable: true });
      (window as unknown as { __keyboard: (height: number) => void }).__keyboard = (height) => {
        keyboard = height;
        fake.dispatchEvent(new Event("resize"));
      };
    });
    await app.goto(`/chat/${BOB}`);
    await playFromTheChat(app, app.getByTestId("apps"));
    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(300));
    await expect.poll(() => app.evaluate(() => document.documentElement.classList.contains("ft-keyboard-open"))).toBe(true);
    await laidOut(app, { width: 360, height: 740 - 300 });
    await shot(app, "phone-keyboard-dark");
  });

  // Seen on the Samsung (2026-10-02): with the keyboard open the user scrolls the board in the
  // small area; once the message is sent and the keyboard goes, the game is shown from its top
  // again (its score and status), not left scrolled down.
  test("the game is shown from its top again when the keyboard goes", async ({ app }) => {
    await app.addInitScript(() => {
      const real = window.visualViewport!;
      let keyboard = 0;
      const fake = new EventTarget();
      Object.defineProperties(fake, {
        height: { get: () => window.innerHeight - keyboard },
        offsetTop: { get: () => 0 },
        width: { get: () => real.width },
        offsetLeft: { get: () => 0 },
        pageTop: { get: () => 0 },
        pageLeft: { get: () => 0 },
        scale: { get: () => 1 },
      });
      Object.defineProperty(window, "visualViewport", { value: fake, configurable: true });
      (window as unknown as { __keyboard: (height: number) => void }).__keyboard = (height) => {
        keyboard = height;
        fake.dispatchEvent(new Event("resize"));
      };
    });
    await app.goto(`/chat/${BOB}`);
    await playFromTheChat(app, app.getByTestId("apps"));
    // A game taller than the room, as a chess board with its score is on a phone.
    await frameSays(app, { type: "ft.height", height: 900 });
    const area = app.getByTestId("game-area");
    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(300));
    await expect.poll(() => app.evaluate(() => document.documentElement.classList.contains("ft-keyboard-open"))).toBe(true);
    await area.evaluate((element) => (element.scrollTop = 160));
    expect(await area.evaluate((element) => element.scrollTop)).toBeGreaterThan(100);

    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(0));
    await expect.poll(() => app.evaluate(() => document.documentElement.classList.contains("ft-keyboard-open"))).toBe(false);
    await expect.poll(() => area.evaluate((element) => element.scrollTop)).toBe(0);
  });

  // 📞 goes to the call screen as from any chat; back in the chat, the game is still there and
  // still the same game (its page was never reloaded), and the call bar leaves its controls alone.
  test("a call and back: the game still running, the call bar clear of the room", async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as { __ftFakeNative: boolean }).__ftFakeNative = true));
    await app.goto(`/chat/${BOB}`);
    await playFromTheChat(app, app.getByTestId("apps"));
    await expect.poll(async () => (await frameHeard(app)).length).toBeGreaterThan(0);
    const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
    await frame.evaluate(() => ((window as unknown as { mark: string }).mark = "still me"));

    await app.getByRole("button", { name: "Voice call", exact: true }).click();
    await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
    await app.goBack();
    await expect(app.getByTestId("call-bar")).toBeVisible();
    await expect(app.getByTestId("game-room")).toBeVisible();
    // The chat slides back in (page-transition.ts, 2026-10-09); it is measured once in place, when
    // Ionic gives the page its taps back.
    await expect(app.locator("div.ion-page", { has: app.getByTestId("game-room") })).not.toHaveCSS("pointer-events", "none");
    expect(await frame.evaluate(() => (window as unknown as { mark?: string }).mark)).toBe("still me");

    const callBar = await box(app.getByTestId("call-bar"));
    // On a phone the bar has its own band above the header (2026-10-03): the contact's name and
    // status are not under it either.
    for (const control of [app.getByTestId("peer"), app.getByTestId("close-game"), app.getByTestId("game-invite"), app.getByTestId("game-bar")]) {
      expect(overlap(callBar, await box(control))).toBe(false);
    }
    await laidOut(app, { width: 360, height: 740 });
    await shot(app, "phone-room-call-dark");
  });
});
