// Android edge-to-edge (2026-10-03, seen on a Samsung): the system's navigation bar sits over the
// bottom of the WebView, and the phone says how tall it is in `--ion-safe-area-bottom` (48 px
// there). A bottom sheet keeps its last row above it, as the composer does, so it can be tapped.
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const INSET = 48;

test.use({ viewport: { width: 360, height: 853 } });

test.beforeEach(async ({ app }) => {
  // As the phone reports it. Set once the page exists: an init script runs before <html> does.
  await app.addInitScript((inset) => {
    document.addEventListener("DOMContentLoaded", () => document.documentElement.style.setProperty("--ion-safe-area-bottom", `${inset}px`));
  }, INSET);
});

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> FT_SHOTS_TAG=<tag> npx playwright test e2e/safe-area.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(300);
  await app.screenshot({ path: `${dir}/${name}-${process.env.FT_SHOTS_TAG ?? "now"}.png` });
}

/** The row stands wholly above the system's bar, once whatever carries it has stopped moving. */
async function aboveTheBar(app: Page, row: Locator) {
  let last = "";
  await expect
    .poll(async () => {
      const now = JSON.stringify(await row.boundingBox());
      const still = now === last;
      last = now;
      return still;
    }, { intervals: [100] })
    .toBe(true);
  const box = (await row.boundingBox())!;
  expect(box.y + box.height, `the row ends at ${box.y + box.height}, under the bar`).toBeLessThanOrEqual(app.viewportSize()!.height - INSET);
}

test("the chat's apps sheet keeps its last row above the navigation bar, on both segments", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  await expect(app.getByTestId("app-com.flickertalk.sketch")).toBeVisible();
  await shot(app, "apps-tab");
  await aboveTheBar(app, app.locator("[data-test='apps-sheet-tools'] ion-item").last());

  await app.getByTestId("apps-tab-games").click();
  await expect(app.getByTestId("more-games-link")).toBeVisible();
  await shot(app, "games-tab");
  await aboveTheBar(app, app.getByTestId("more-games-link"));
});

test("the games' permissions sheet and contact picker stay above the navigation bar", async ({ app }) => {
  await app.goto("/tabs/games");
  await app.getByTestId("play-com.flickertalk.game.tictactoe").click();
  await expect(app.getByTestId("game-allow")).toBeVisible();
  await shot(app, "permissions-sheet");
  await aboveTheBar(app, app.getByTestId("game-allow"));

  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
  await shot(app, "contact-picker");
  await aboveTheBar(app, app.getByTestId(`play-with-${BOB}`));
});

test("the permissions sheet opened from a chat stays above the navigation bar", async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__ftFakeBobSays = ["🎮 Shall we play Chess? https://flickertalk.com/games/chess"];
  });
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("play-game").click();
  await expect(app.getByTestId("game-allow")).toBeVisible();
  await shot(app, "permissions-sheet-in-chat");
  await aboveTheBar(app, app.getByTestId("game-allow"));
});

// Ioan, 2026-10-03: "asegúrate que tenga scroll". With more plugins or games than fit, the sheet's
// list scrolls, and its last item can be brought above the bar and tapped.
test.describe("with many plugins and games", () => {
  test.use({ viewport: { width: 360, height: 740 } });
  test.beforeEach(async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 30));
  });

  /** Scrolls the open sheet's content to its end, as a finger would. */
  async function scrollToEnd(app: Page) {
    await app.locator("ion-modal ion-content").evaluate((content) => (content as HTMLIonContentElement).scrollToBottom(0));
    await app.waitForTimeout(150);
  }

  test("the last plugin and the last game can be scrolled to and tapped", async ({ app }) => {
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    const lastTool = app.getByTestId("app-com.example.tool29");
    await expect(app.getByTestId("app-com.example.tool00")).toBeVisible();
    await shot(app, "many-plugins");
    await scrollToEnd(app);
    await expect(lastTool).toBeInViewport({ ratio: 1 });
    await aboveTheBar(app, lastTool);
    await shot(app, "many-plugins-end");
    await lastTool.click();
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);

    await app.getByTestId("close-app").click();
    await app.getByTestId("apps").click();
    await app.getByTestId("apps-tab-games").click();
    await expect(app.getByTestId("game-com.flickertalk.game.many00")).toBeVisible();
    await scrollToEnd(app);
    const more = app.getByTestId("more-games-link");
    await expect(more).toBeInViewport({ ratio: 1 });
    await aboveTheBar(app, more);
    await shot(app, "many-games-end");
    await app.getByTestId("game-com.flickertalk.game.many29").click();
    // Never played: it asks first.
    await expect(app.getByTestId("game-permissions")).toBeVisible();
  });
});
