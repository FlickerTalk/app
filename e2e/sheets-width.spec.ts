// The sheets on a wide screen (decided 2026-10-02): each covers the pane the user is looking at,
// bottom-anchored, never Ionic's 600 px floating over the whole window. Beside the list it is the
// chat pane; on the games tab, everything after the rail; a conversation open on its own, the
// whole window. A phone keeps the whole width. The direction follows the language (RTL in Arabic).
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

type Box = { x: number; y: number; width: number; height: number };

/**
 * Screenshots for review, only when asked for: `FT_SHOTS=<dir> [FT_APPEARANCE=light] npx playwright
 * test e2e/sheets-width.spec.ts` (dark is the app's default).
 */
const APPEARANCE = process.env.FT_APPEARANCE ?? "dark";
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(450);
  await app.screenshot({ path: `${dir}/${name}-${APPEARANCE}.png` });
}

test.beforeEach(async ({ app }) => {
  await app.addInitScript((chosen) => localStorage.setItem("ft-appearance", chosen), APPEARANCE);
});

/** A sheet's box, once Ionic has finished bringing it up. */
async function settled(wrapper: Locator): Promise<Box> {
  let last = "";
  await expect
    .poll(async () => {
      const now = JSON.stringify(await wrapper.boundingBox());
      const still = now === last && now !== "null";
      last = now;
      return still;
    }, { intervals: [100] })
    .toBe(true);
  return (await wrapper.boundingBox())!;
}

/** The sheet covers [from, to) across, within a pixel, and sits at the bottom. */
function covers(box: Box, from: number, to: number, height: number) {
  expect(Math.abs(box.x - from), `starts at ${from}, not ${box.x}`).toBeLessThanOrEqual(1);
  expect(Math.abs(box.x + box.width - to), `ends at ${to}, not ${box.x + box.width}`).toBeLessThanOrEqual(1);
  // At a part of its height Ionic slides a full-height sheet down: it reaches the bottom, or beyond.
  expect(box.y + box.height, "bottom-anchored").toBeGreaterThanOrEqual(height - 1);
}

const TABLETS = [
  { name: "1280", width: 1280, height: 800 },
  // The Lenovo: 2560×1600 at its pixel ratio.
  { name: "lenovo", width: 1400, height: 875 },
];

for (const tablet of TABLETS) {
  for (const locale of ["en", "ar"]) {
    test.describe(`${tablet.name}, ${locale}`, () => {
      test.use({ viewport: { width: tablet.width, height: tablet.height }, locale });

      test("beside the list, the apps sheet covers the chat pane and leaves the list alone", async ({ app }) => {
        await app.goto("/tabs/chats");
        await app.locator(".ft-chats__detail [data-test='apps']").click();
        const sheet = await settled(app.locator("ion-modal.ft-apps-sheet .modal-wrapper"));
        const pane = (await app.locator(".ft-chats__detail").boundingBox())!;
        const list = (await app.locator(".ft-chats__list").boundingBox())!;
        // In Arabic the pane is on the left: the sheet ends where the window starts.
        covers(sheet, pane.x, pane.x + pane.width, tablet.height);
        expect(sheet.x >= list.x + list.width - 1 || sheet.x + sheet.width <= list.x + 1, "not over the list").toBe(true);
        if (locale === "ar") expect(Math.round(pane.x)).toBe(0);
        await shot(app, `${tablet.name}-${locale}-apps-sheet`);

        // The permissions sheet of a game opened from that chat covers the same pane.
        await app.getByTestId("apps-tab-games").click();
        await app.getByTestId(`game-${TICTACTOE}`).click();
        const ask = await settled(app.locator("ion-modal.ft-game-ask .modal-wrapper"));
        covers(ask, pane.x, pane.x + pane.width, tablet.height);
        await shot(app, `${tablet.name}-${locale}-permissions-in-chat`);
      });

      test("on the Apps tab, the sheets cover everything after the rail", async ({ app }) => {
        await app.goto("/tabs/apps?show=games");
        const rail = (await app.locator("nav.ft-rail").boundingBox())!;
        const [from, to] = locale === "ar" ? [0, rail.x] : [rail.x + rail.width, tablet.width];
        // An app's sheet (a hold on its tile), then a game's two sheets.
        await app.getByTestId(`app-${TICTACTOE}`).click({ button: "right" });
        covers(await settled(app.locator("ion-modal.ft-app-sheet .modal-wrapper")), from, to, tablet.height);
        await shot(app, `${tablet.name}-${locale}-app-sheet`);
        await app.getByTestId("sheet-play").click();
        covers(await settled(app.locator("ion-modal.ft-game-ask .modal-wrapper")), from, to, tablet.height);
        await shot(app, `${tablet.name}-${locale}-games-permissions`);
        await app.getByTestId("game-allow").click();
        covers(await settled(app.locator("ion-modal.ft-apps__picker .modal-wrapper")), from, to, tablet.height);
        await shot(app, `${tablet.name}-${locale}-games-picker`);
      });

      test("a conversation open on its own: the sheet spans the window", async ({ app }) => {
        await app.goto(`/chat/${BOB}`);
        await app.getByTestId("apps").click();
        covers(await settled(app.locator("ion-modal.ft-apps-sheet .modal-wrapper")), 0, tablet.width, tablet.height);
        await shot(app, `${tablet.name}-${locale}-chat-full-screen`);
      });
    });
  }
}

test.describe("a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("keeps the whole width for every sheet", async ({ app }) => {
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    covers(await settled(app.locator("ion-modal.ft-apps-sheet .modal-wrapper")), 0, 360, 740);
    await app.goto("/tabs/apps?show=games");
    await app.getByTestId(`app-${TICTACTOE}`).click();
    covers(await settled(app.locator("ion-modal.ft-game-ask .modal-wrapper")), 0, 360, 740);
    await app.getByTestId("game-allow").click();
    covers(await settled(app.locator("ion-modal.ft-apps__picker .modal-wrapper")), 0, 360, 740);
  });
});
