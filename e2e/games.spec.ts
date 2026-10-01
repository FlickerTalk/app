// Plan 10 (app 1.3.0): the games section. A game is a plugin of the signed catalogue that plays in
// a conversation, with the live channel to the same game on the other phone. The fake core has one
// game installed (Tic-tac-toe, granted nothing yet) and offers another (Chess).
import type { Page } from "@playwright/test";
import { callsTo, expect, frameHeard, servePluginFrames, test } from "./helpers";

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

    test("the games tab lists my games and the catalogue's, and installs one", async ({ app }) => {
      await app.goto("/tabs/chats");
      await app.getByRole("tab", { name: "Games" }).click();
      await expect(app).toHaveURL(/\/tabs\/games$/);
      await expect(app.getByTestId("my-games")).toContainText("Tic-tac-toe");
      await expect(app.getByTestId("more-games")).toContainText("Chess");
      await expect(app.getByTestId("more-games")).toContainText("412 KB");
      // Tools stay in Settings.
      await expect(app.getByTestId("my-games")).not.toContainText("Markdown");
      await shot(app, "games-tab");

      await app.getByTestId(`install-${CHESS}`).click();
      await expect(app.getByTestId("my-games")).toContainText("Chess");
      await expect(app.getByTestId("more-games")).toHaveCount(0);
      expect((await callsTo(app)).some(([command, args]) => command === "core_plugin_add" && args?.plugin === CHESS)).toBe(true);
    });

    test("with no game installed, the tab says so", async ({ app }) => {
      await app.goto("/tabs/games");
      await app.getByTestId(`remove-${TICTACTOE}`).click();
      await expect(app.getByTestId("my-games")).toContainText("Removing it deletes its saved games on this phone.");
      await app.getByTestId("remove-confirm").click();
      await expect(app.getByText("No games yet")).toBeVisible();
      await shot(app, "games-tab-empty");
    });

    test("a game from the tab asks once, then who to play with, and opens in that chat with the live channel", async ({ app }) => {
      await app.goto("/tabs/games");
      await app.getByTestId(`play-${TICTACTOE}`).click();
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

    test("the chat's games sheet invites without sending, and leads to more games", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      // The tools button shows tools only.
      await app.getByTestId("apps").click();
      await expect(app.getByTestId(`app-${TICTACTOE}`)).toHaveCount(0);
      await app.locator(".ft-apps").click({ position: { x: 10, y: 10 } });

      await app.getByTestId("games").click();
      await expect(app.getByTestId("games-sheet")).toContainText("Tic-tac-toe");
      await shot(app, "chat-games-sheet");
      await app.getByTestId(`invite-${TICTACTOE}`).click();
      await expect(app.locator("ion-textarea textarea")).toHaveValue("🎮 Shall we play Tic-tac-toe? https://flickertalk.com/games/tictactoe");
      expect((await callsTo(app)).some(([command]) => command === "core_send")).toBe(false);

      await app.getByTestId("games").click();
      await app.getByTestId("more-games-link").click();
      await expect(app).toHaveURL(/\/tabs\/games$/);
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
  await expect(app.getByTestId("games")).toBeVisible();
  expect((await callsTo(app)).some(([command]) => command === "core_catalogue")).toBe(false);
});

test("an install that fails says so", async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeInstallFails = true));
  await app.goto("/tabs/games");
  await app.getByTestId(`install-${CHESS}`).click();
  await expect(app.getByRole("alert")).toHaveText("The game could not be installed. Check your connection and try again.");
});

test.describe("on an iPhone", () => {
  test.use({ userAgent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148" });

  // No downloads on iOS (App Store 4.7, §52): no games tab, no games button in a chat.
  test("there are no games", async ({ app }) => {
    await app.goto(`/chat/${BOB}`);
    await expect(app.getByTestId("peer")).toBeVisible();
    await expect(app.getByTestId("games")).toHaveCount(0);
    await app.goto("/tabs/chats");
    await expect(app.getByRole("tab", { name: "Calls" })).toBeVisible();
    await expect(app.getByRole("tab", { name: "Games" })).toHaveCount(0);
  });
});
