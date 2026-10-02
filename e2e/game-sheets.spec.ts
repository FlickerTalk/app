// The games' two sheets (Ioan, 2026-10-02: Ionic's components wherever one exists, icons from
// ion-icon, never emoji): the permissions sheet of the first play and the contact picker are
// Ionic's sheet modals, with a name, closed by Android's back button, as the apps sheet is.
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test e2e/game-sheets.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(450);
  await app.screenshot({ path: `${dir}/${name}.png` });
}

const back = (app: Page) => app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());

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
      });

      test("from the games tab: the permissions sheet, then the contact picker", async ({ app }) => {
        await app.goto("/tabs/games");
        await shot(app, `${size.name}-games-tab-${appearance}`);
        await app.getByTestId(`play-${TICTACTOE}`).click();
        const ask = app.getByRole("dialog", { name: "Tic-tac-toe" });
        await expect(ask).toBeVisible();
        await expect(app.getByTestId("game-permissions").locator("ion-icon")).toBeVisible();
        await expect(app.getByTestId("game-permissions")).not.toContainText("🎮");
        await shot(app, `${size.name}-permissions-${appearance}`);

        await app.getByTestId("game-allow").click();
        await expect(app.getByRole("dialog", { name: "Play with" })).toBeVisible();
        await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
        await shot(app, `${size.name}-picker-${appearance}`);
        await app.getByTestId(`play-with-${BOB}`).click();
        await expect(app.getByTestId("game-room")).toBeVisible();
      });

      test("from an invitation: install and play, in the same sheet", async ({ app }) => {
        await app.addInitScript(() => {
          (window as unknown as Record<string, unknown>).__ftFakeBobSays = ["🎮 Shall we play Chess? https://flickertalk.com/games/chess"];
        });
        await app.goto(size.width < 768 ? `/chat/${BOB}` : "/tabs/chats");
        const play = app.getByTestId("play-game").last();
        await expect(play).toBeVisible();
        await shot(app, `${size.name}-bubble-play-${appearance}`);
        await play.click();
        await expect(app.getByRole("dialog", { name: "Chess" })).toBeVisible();
        await expect(app.getByTestId("game-allow")).toContainText("Install and play");
        await shot(app, `${size.name}-install-and-play-${appearance}`);
        await app.getByTestId("game-allow").click();
        await expect(app.getByTestId("game-room")).toBeVisible();
      });
    });
  }
}

test.describe("the back button", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("closes the permissions sheet and the picker, and stays on the games", async ({ app }) => {
    await app.goto("/tabs/games");
    await app.getByTestId(`play-${TICTACTOE}`).click();
    await expect(app.getByRole("dialog", { name: "Tic-tac-toe" })).toBeVisible();
    expect(await back(app)).toBe(true);
    await expect(app.getByTestId("game-permissions")).toBeHidden();
    // Refused that way, nothing was granted.
    await app.getByTestId(`play-${TICTACTOE}`).click();
    await app.getByTestId("game-allow").click();
    await expect(app.getByRole("dialog", { name: "Play with" })).toBeVisible();
    expect(await back(app)).toBe(true);
    await expect(app.getByTestId("contact-picker")).toBeHidden();
    await expect(app).toHaveURL(/\/tabs\/games$/);
    expect(await back(app)).toBe(false);
  });

  test("closes the permissions sheet opened from an invitation, and stays in the chat", async ({ app }) => {
    await app.addInitScript(() => {
      (window as unknown as Record<string, unknown>).__ftFakeBobSays = ["🎮 Shall we play Chess? https://flickertalk.com/games/chess"];
    });
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("play-game").click();
    await expect(app.getByRole("dialog", { name: "Chess" })).toBeVisible();
    expect(await back(app)).toBe(true);
    await expect(app.getByTestId("game-permissions")).toBeHidden();
    await expect(app.getByTestId("game-room")).toHaveCount(0);
    await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
  });
});
