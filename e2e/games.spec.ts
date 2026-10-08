// Plan 10 (app 1.3.0): the games section. A game is a plugin of the signed catalogue that plays in
// a conversation, with the live channel to the same game on the other phone. The fake core has one
// game installed (Tic-tac-toe, granted nothing yet) and offers another (Chess).
import type { Page } from "@playwright/test";
import { callsTo, expect, frameHeard, holdTile, servePluginFrames, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";
const CHESS = "com.flickertalk.game.chess";

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test e2e/games.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  const appearance = await app.evaluate(() => localStorage.getItem("ft-appearance") ?? "dark");
  await app.waitForTimeout(350);
  await app.screenshot({ path: `${dir}/${name}-${appearance}.png` });
}

/** The live channel the open game was handed, as its frame heard it. */
async function openedLive(app: Page): Promise<boolean | undefined> {
  await expect.poll(async () => (await frameHeard(app)).length).toBeGreaterThan(0);
  const opened = (await frameHeard(app)).find((one) => (one as { type?: string }).type === "ft.open") as { live?: boolean } | undefined;
  return opened?.live;
}

for (const appearance of ["dark", "light"]) {
  test.describe(`in ${appearance}`, () => {
    test.beforeEach(async ({ app }) => {
      await app.addInitScript((chosen) => localStorage.setItem("ft-appearance", chosen), appearance);
      await servePluginFrames(app);
    });

    // 2026-10-08 (plan of the apps grid): the games are a segment of the Apps tab, as tiles.
    test("the Apps tab lists my games and the catalogue's, and installs one", async ({ app }) => {
      await app.goto("/tabs/chats");
      await app.getByRole("tab", { name: "Apps" }).click();
      await expect(app).toHaveURL(/\/tabs\/apps$/);
      await app.getByTestId("apps-segment-games").click();
      await expect(app.getByTestId("apps-installed")).toContainText("Tic-tac-toe");
      await expect(app.getByTestId("apps-more")).toContainText("Chess");
      await expect(app.getByTestId("apps-more")).toContainText("412 KB");
      // The tools have their own segment.
      await expect(app.getByTestId("apps-installed")).not.toContainText("Markdown");
      await shot(app, "games-tab");

      await app.getByTestId(`install-${CHESS}`).click();
      await expect(app.getByTestId("apps-installed")).toContainText("Chess");
      await expect(app.getByTestId("apps-more")).toHaveCount(0);
      await expect(app.getByTestId("apps-all-here")).toHaveText("All the games are on this phone");
      expect((await callsTo(app)).some(([command, args]) => command === "core_plugin_add" && args?.plugin === CHESS)).toBe(true);
    });

    // A hold shows the game's sheet; removing asks once, can be cancelled, and is forgotten when
    // the sheet goes.
    test("a game is removed from its sheet after a warning, and then the tab says there is none", async ({ app }) => {
      await app.goto("/tabs/apps?show=games");
      await holdTile(app, `app-${TICTACTOE}`);
      await app.getByTestId("sheet-remove").click();
      await expect(app.getByTestId("app-sheet")).toContainText("Removing it deletes its saved games on this phone.");
      await shot(app, "games-remove-ask");
      expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(true);
      await expect(app.getByTestId("app-sheet")).toBeHidden();
      await holdTile(app, `app-${TICTACTOE}`);
      await expect(app.getByTestId("remove-confirm")).toHaveCount(0);
      await app.getByTestId("sheet-remove").click();
      await app.getByTestId("remove-cancel").click();
      await expect(app.getByTestId("remove-confirm")).toHaveCount(0);

      await app.getByTestId("sheet-remove").click();
      await app.getByTestId("remove-confirm").click();
      await expect(app.getByTestId("app-sheet")).toBeHidden();
      await expect(app.getByTestId("apps-none")).toHaveText("No games yet");
      expect((await callsTo(app)).some(([command, args]) => command === "core_plugin_remove" && args?.plugin === TICTACTOE)).toBe(true);
      await shot(app, "games-tab-empty");
    });

    test("a game from the tab asks once, then who to play with, and opens in that chat with the live channel", async ({ app }) => {
      await app.goto("/tabs/apps?show=games");
      await app.getByTestId(`app-${TICTACTOE}`).click();
      await expect(app.getByTestId("game-permissions")).toContainText("This game talks to the other person's phone");
      await shot(app, "permissions-sheet");
      await app.getByTestId("game-allow").click();

      await expect(app.getByTestId("contact-picker")).toContainText("Bob");
      await shot(app, "contact-picker");
      await app.getByTestId(`play-with-${BOB}`).click();

      await expect(app).toHaveURL(new RegExp(`/chat/${BOB}\\?play=${TICTACTOE.replace(/\./g, "\\.")}$`));
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      expect(await openedLive(app)).toBe(true);
      const granted = (await callsTo(app)).find(([command]) => command === "core_plugin_grant");
      expect(granted?.[1]).toMatchObject({ plugin: TICTACTOE, granted: { live: true, send: "propose" } });
    });

    // Ioan, 2026-10-02: one apps button in the header; it opens a sheet modal with a segment for the
    // plugins and one for the games.
    test("the chat's apps sheet has the tools and the games apart; a held game invites without sending", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await expect(app.getByTestId("games")).toHaveCount(0);
      await app.getByTestId("apps").click();
      // With tools installed it opens on them, and they are tools only.
      await expect(app.getByTestId("apps-tab-tools")).toHaveClass(/segment-button-checked/);
      await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeVisible();
      await expect(app.getByTestId(`app-${TICTACTOE}`)).toHaveCount(0);
      await shot(app, "chat-apps-tab");

      await app.getByTestId("apps-tab-games").click();
      await expect(app.getByTestId("games-sheet")).toContainText("Tic-tac-toe");
      await expect(app.getByTestId("app-com.flickertalk.markdown")).toHaveCount(0);
      await shot(app, "chat-games-sheet");
      await expect(app.getByTestId(`game-${TICTACTOE}`)).toHaveAttribute("aria-label", "Tic-tac-toe. Touch and hold to invite");
      await app.getByTestId(`game-${TICTACTOE}`).click({ button: "right" });
      await expect(app.locator("ion-textarea textarea")).toHaveValue("🎮 Tic-tac-toe · Shall we play? https://flickertalk.com/games/tictactoe");
      expect((await callsTo(app)).some(([command]) => command === "core_send")).toBe(false);

      // Opened again, it starts on the tools: nothing is remembered.
      await app.getByTestId("apps").click();
      await expect(app.getByTestId("apps-tab-tools")).toHaveClass(/segment-button-checked/);
      await app.getByTestId("apps-tab-games").click();
      await app.getByTestId("more-games-link").click();
      await expect(app).toHaveURL(/\/tabs\/apps\?show=games$/);
      await expect(app.getByTestId("apps-segment-games")).toHaveClass(/segment-button-checked/);
    });

    test("an invitation to a game of the catalogue installs it, asks, and opens it; a look-alike gets nothing", async ({ app }) => {
      await app.addInitScript(() => {
        (window as unknown as Record<string, unknown>).__ftFakeBobSays = [
          "🎮 Shall we play Chess? https://flickertalk.com/games/chess",
          "https://flickertalk.com.evil.example/games/chess",
        ];
      });
      await app.goto(`/chat/${BOB}`);
      await expect(app.getByTestId("play-game")).toHaveCount(1);
      await expect(app.getByTestId("play-game")).toContainText("Chess · 412 KB");
      await shot(app, "bubble-play");

      await app.getByTestId("play-game").click();
      await expect(app.getByTestId("game-allow")).toHaveText("Install and play");
      await app.getByTestId("game-allow").click();
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      expect(await openedLive(app)).toBe(true);
      const calls = await callsTo(app);
      expect(calls.some(([command, args]) => command === "core_plugin_add" && args?.plugin === CHESS)).toBe(true);
      expect(calls.some(([command, args]) => command === "core_plugin_grant" && args?.plugin === CHESS)).toBe(true);
    });
  });
}

test("a chat without invitations does not read the catalogue", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await expect(app.getByTestId("apps")).toBeVisible();
  expect((await callsTo(app)).some(([command]) => command === "core_catalogue")).toBe(false);
});

// A toast says it, over the grid: nothing in the grid moves (no layout shift).
test("an install that fails says so, and moves nothing", async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeInstallFails = true));
  await app.goto("/tabs/apps?show=games");
  const tile = app.getByTestId(`install-${CHESS}`);
  await expect(tile).toBeVisible();
  const before = await tile.boundingBox();
  await tile.click();
  await expect(app.getByText("It could not be installed. Check your connection and try again.")).toBeVisible();
  expect(await tile.boundingBox()).toEqual(before);
});

test.describe("on an iPhone", () => {
  test.use({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148" });

  // 2026-10-03: nothing is downloaded on iOS (App Store 4.7, §52), but the app carries the games,
  // so an iPhone has the Apps tab and the games segment of the chat's apps like any phone.
  test("there are games", async ({ app }) => {
    await app.goto(`/chat/${BOB}`);
    await expect(app.getByTestId("peer")).toBeVisible();
    await app.getByTestId("apps").click();
    await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeVisible();
    await app.getByTestId("apps-tab-games").click();
    await expect(app.getByTestId("games-sheet")).toBeVisible();
    await app.goto("/tabs/chats");
    await expect(app.getByRole("tab", { name: "Calls" })).toBeVisible();
    await expect(app.getByRole("tab", { name: "Apps" })).toBeVisible();
  });
});
