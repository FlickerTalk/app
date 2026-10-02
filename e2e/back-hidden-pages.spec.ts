// Android's back button and the pages Ionic keeps mounted underneath (seen on the Samsung,
// 2026-10-02): something left open in a page that is no longer on screen (a game in a chat, say)
// must not take the button. Back acts on what is on screen; the page underneath gets it back, with
// what it had open, when it is on screen again.
import type { Page } from "@playwright/test";
import { expect, frameAsks, servePluginFrames, test } from "./helpers";

/** Android's back button: the app's own handling if it listens, the system's (the WebView's history) if not. */
async function pressBack(app: Page): Promise<"app" | "system"> {
  const heard = await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());
  if (heard) return "app";
  await app.goBack();
  return "system";
}

/** The page back on screen has taken the button again (once Ionic's transition has ended). */
async function takenAgain(app: Page) {
  await expect.poll(() => app.evaluate(() => (window as unknown as { __ftFake: { listening: () => boolean } }).__ftFake.listening())).toBe(true);
}

async function openBobsChat(app: Page) {
  await app.goto("/tabs/chats");
  await app.locator(".ft-row", { hasText: "see you at six" }).first().click();
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
}

async function playTicTacToe(app: Page, apps = app.getByTestId("apps")) {
  await apps.click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId("game-com.flickertalk.game.tictactoe").click();
  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId("game-room")).toBeVisible();
}

test.describe("on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("a game left open in a chat does not take Back on Settings → Plugins", async ({ app }) => {
    await openBobsChat(app);
    await playTicTacToe(app);
    await app.getByTestId("apps").click();
    await app.getByTestId("apps-tab-games").click();
    await app.getByTestId("more-games-link").click();
    await expect(app).toHaveURL(/\/tabs\/games$/);
    await app.getByRole("tab", { name: "Settings" }).click();
    await app.getByTestId("plugins").click();
    await expect(app).toHaveURL(/\/plugins$/);
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/tabs\/settings$/);
  });

  test("on the contact page over a running game, Back leaves the page; in the chat it closes the game", async ({ app }) => {
    await openBobsChat(app);
    await playTicTacToe(app);
    await app.getByTestId("peer").click();
    await expect(app).toHaveURL(/\/contact\/ft_bob123456789$/);
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
    await expect(app.getByTestId("game-room")).toBeVisible();
    await takenAgain(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app.getByTestId("game-room")).toHaveCount(0);
    await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  });

  // Existed before the games: a plugin's ft.openChat opens another conversation over the first.
  test("after a plugin opens another chat, Back acts on that chat", async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeCarol = true));
    await servePluginFrames(app);
    await openBobsChat(app);
    await app.getByTestId("apps").click();
    await app.getByTestId("app-com.flickertalk.markdown").click();
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
    await frameAsks(app, { type: "ft.openChat", id: "q1", ref: "r1" });
    await expect(app).toHaveURL(/\/chat\/ft_carol12345678$/);
    // Nothing open in Carol's chat: Back leaves it.
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
    // Bob's tool is still there, and Back closes it.
    await expect(app.locator(".ft-app")).toBeVisible();
    await takenAgain(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app.locator(".ft-app")).toHaveCount(0);
  });
});

test.describe("on a tablet", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("a game in the split pane lets go of Back on another tab, and takes it again on Chats", async ({ app }) => {
    await app.goto("/tabs/chats");
    await playTicTacToe(app, app.locator(".ft-chats__detail [data-test='apps']"));
    await app.getByRole("button", { name: "Settings", exact: true }).click();
    await expect(app).toHaveURL(/\/tabs\/settings$/);
    await app.getByTestId("plugins").click();
    await expect(app).toHaveURL(/\/plugins$/);
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/tabs\/settings$/);
    await app.getByRole("button", { name: "Chats", exact: true }).click();
    await expect(app.getByTestId("game-room")).toBeVisible();
    await takenAgain(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app.getByTestId("game-room")).toHaveCount(0);
  });
});
