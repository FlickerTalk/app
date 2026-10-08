// 2026-10-08 ("Imagen por plugin"): a plugin's own icon.svg is its tile, drawn as an image, in the
// Apps tab, on its sheet and in the chat's apps sheet. The fake core gives Markdown (a tool) and
// Tic-tac-toe (a game) an image; Sketch has none and keeps its Ionicon.
import type { Locator } from "@playwright/test";
import { expect, holdTile, test } from "./helpers";

const MARKDOWN = "com.flickertalk.markdown";
const SKETCH = "com.flickertalk.sketch";
const TICTACTOE = "com.flickertalk.game.tictactoe";

/** The image in a box has loaded and fills it. */
async function drawn(box: Locator) {
  const img = box.locator("img");
  await expect(img).toHaveAttribute("src", /^data:image\/svg\+xml;base64,/);
  await expect.poll(() => img.evaluate((el: HTMLImageElement) => el.complete && el.naturalWidth > 0)).toBe(true);
  const [outer, inner] = [await box.boundingBox(), await img.boundingBox()];
  expect(Math.round(inner!.width)).toBe(Math.round(outer!.width));
  await expect(box.locator("ion-icon")).toHaveCount(0);
}

test("a plugin's own image is its tile in the Apps tab, on its sheet and in the chat", async ({ app }) => {
  await app.goto("/tabs/apps");
  await drawn(app.getByTestId(`app-${MARKDOWN}`).locator(".ft-app-tile__icon"));
  await expect(app.getByTestId(`app-${SKETCH}`).locator(".ft-app-tile__icon img")).toHaveCount(0);
  await expect(app.getByTestId(`app-${SKETCH}`).locator(".ft-app-tile__icon ion-icon")).toBeVisible();

  await holdTile(app, `app-${MARKDOWN}`);
  await drawn(app.locator(".ft-app-sheet__icon"));

  await app.goto("/tabs/apps?show=games");
  await drawn(app.getByTestId(`app-${TICTACTOE}`).locator(".ft-app-tile__icon"));
  await app.getByTestId(`app-${TICTACTOE}`).click();
  await expect(app.getByTestId("game-permissions").locator("img")).toBeVisible();

  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await drawn(app.getByTestId(`app-${MARKDOWN}`).locator(".ft-app-tile__icon"));
  await expect(app.getByTestId(`app-${SKETCH}`).locator(".ft-app-tile__icon img")).toHaveCount(0);
  await app.getByTestId("apps-tab-games").click();
  await drawn(app.getByTestId(`game-${TICTACTOE}`).locator(".ft-app-tile__icon"));
});
