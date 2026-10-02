// A phone held sideways (2026-10-02, seen on an iPhone 17 Pro simulator, 874×402): the Dynamic
// Island, a notch or a camera cutout sits at one side and the system reports the side insets
// (62 px there, the same on both sides). Nothing that can be read or tapped goes under them; the
// backgrounds still reach the edges. At 874 px the app shows the rail, the list and the chat side
// by side, and the sides that touch no edge keep no inset. The island does not move with the
// language: in Arabic the layout is mirrored, the insets are not.
//
// The phone reports the insets through `env(safe-area-inset-*)`, which Ionic turns into
// `--ion-safe-area-*` on the root; Chromium cannot fake `env()`, so the test sets those variables,
// as `safe-area.spec.ts` does for the bottom.
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";
const WIDTH = 874;
const HEIGHT = 402;
const SIDE = 62;

type Box = { x: number; y: number; width: number; height: number };

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> FT_SHOTS_TAG=<tag> npx playwright test e2e/safe-area-sides.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(450);
  await app.screenshot({ path: `${dir}/${name}-${process.env.FT_SHOTS_TAG ?? "now"}.png` });
}

/** Where something is once it has stopped moving (a sheet sliding up, a page coming in). */
async function settled(one: Locator): Promise<Box> {
  let last = "";
  await expect
    .poll(async () => {
      const now = JSON.stringify(await one.boundingBox());
      const still = now === last && now !== "null";
      last = now;
      return still;
    }, { intervals: [100] })
    .toBe(true);
  return (await one.boundingBox())!;
}

/** Every visible match stands between the two side insets. */
async function clearOfTheSides(what: Locator, label: string) {
  await settled(what.first());
  const boxes = (await what.evaluateAll((all) =>
    all
      .map((one) => one.getBoundingClientRect())
      .filter((box) => box.width > 0 && box.height > 0)
      .map((box) => ({ x: box.x, right: box.right })),
  )) as Array<{ x: number; right: number }>;
  expect(boxes.length, `${label}: something to measure`).toBeGreaterThan(0);
  for (const box of boxes) {
    expect(box.x, `${label} starts under the left inset`).toBeGreaterThanOrEqual(SIDE - 0.5);
    expect(box.right, `${label} ends under the right inset`).toBeLessThanOrEqual(WIDTH - SIDE + 0.5);
  }
}

test.use({ viewport: { width: WIDTH, height: HEIGHT } });

test.beforeEach(async ({ app }) => {
  // As the phone reports them. Set once the page exists: an init script runs before <html> does.
  await app.addInitScript((side) => {
    document.addEventListener("DOMContentLoaded", () => {
      document.documentElement.style.setProperty("--ion-safe-area-left", `${side}px`);
      document.documentElement.style.setProperty("--ion-safe-area-right", `${side}px`);
    });
  }, SIDE);
});

for (const locale of ["en", "ar"]) {
  test.describe(locale, () => {
    test.use({ locale });

    test("beside the list: the rail, the bubbles, the composer and the apps sheet stay clear of the sides", async ({ app }) => {
      await app.goto("/tabs/chats");
      const pane = app.locator(".ft-chats__detail");
      await expect(pane.locator("[data-test='apps']")).toBeVisible();
      await shot(app, `${locale}-split`);

      await clearOfTheSides(app.locator("nav.ft-rail button"), "a rail button");
      await clearOfTheSides(pane.locator(".ft-bubble"), "a bubble");
      await clearOfTheSides(pane.locator(".ft-composer__row"), "the composer");

      // The chat pane reaches one edge only: the inline end (the right, or the left in Arabic).
      // Beside the list it keeps Ionic's padding and no more.
      const paneBox = (await pane.boundingBox())!;
      const row = (await pane.locator(".ft-composer__row").boundingBox())!;
      if (locale === "ar") expect(paneBox.x + paneBox.width - (row.x + row.width), "no inset beside the list").toBeLessThan(SIDE);
      else expect(row.x - paneBox.x, "no inset beside the list").toBeLessThan(SIDE);

      await pane.locator("[data-test='apps']").click();
      const sheet = app.locator("ion-modal.ft-apps-sheet");
      await clearOfTheSides(sheet.locator("ion-item ion-icon"), "an apps sheet icon");
      await clearOfTheSides(sheet.locator("ion-item ion-label"), "an apps sheet name");
      // Same pane, same rule: the icons start where Ionic puts them, beside the list.
      const icon = (await sheet.locator("ion-item ion-icon").first().boundingBox())!;
      if (locale === "ar") expect(paneBox.x + paneBox.width - (icon.x + icon.width), "no inset beside the list").toBeLessThan(SIDE);
      else expect(icon.x - paneBox.x, "no inset beside the list").toBeLessThan(SIDE);
      await shot(app, `${locale}-split-apps-sheet`);

      // A tool's window covers the list and the chat.
      await app.getByTestId("app-com.flickertalk.sketch").click();
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await clearOfTheSides(app.getByTestId("close-app"), "the tool's way out");
      await clearOfTheSides(app.locator("iframe.ft-plugin__frame"), "the tool");
      await shot(app, `${locale}-split-tool`);
    });

    test("a conversation on its own: the bubbles, the composer, the apps sheet and a tool stay clear of both sides", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await expect(app.getByTestId("apps")).toBeVisible();
      await clearOfTheSides(app.locator(".ft-bubble"), "a bubble");
      await clearOfTheSides(app.locator(".ft-composer__row"), "the composer");
      await shot(app, `${locale}-alone`);

      await app.getByTestId("apps").click();
      const sheet = app.locator("ion-modal.ft-apps-sheet");
      await clearOfTheSides(sheet.locator("ion-item ion-icon"), "an apps sheet icon");
      await clearOfTheSides(sheet.locator("ion-item ion-label"), "an apps sheet name");
      await shot(app, `${locale}-alone-apps-sheet`);

      await app.getByTestId("app-com.flickertalk.sketch").click();
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await clearOfTheSides(app.getByTestId("close-app"), "the tool's way out");
      await clearOfTheSides(app.locator("iframe.ft-plugin__frame"), "the tool");
      await shot(app, `${locale}-alone-tool`);
    });

    test("the games tab: the permissions sheet and the contact picker stay clear of the sides", async ({ app }) => {
      await app.goto("/tabs/games");
      await app.getByTestId(`play-${TICTACTOE}`).click();
      await expect(app.getByTestId("game-allow")).toBeVisible();
      await clearOfTheSides(app.locator("[data-test='game-permissions'] > *"), "the permissions sheet");
      await shot(app, `${locale}-games-permissions`);

      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
      await clearOfTheSides(app.locator("ion-modal.ft-games__picker ion-item ion-label"), "a contact's name");
      const picture = app.locator("ion-modal.ft-games__picker ion-item [slot='start']");
      await clearOfTheSides(picture, "a contact's picture");
      // The sheet starts past the rail, away from the edge: the pictures start where Ionic puts them.
      const rail = (await app.locator("nav.ft-rail").boundingBox())!;
      const face = (await picture.first().boundingBox())!;
      if (locale === "ar") expect(rail.x - (face.x + face.width), "no inset beside the rail").toBeLessThan(SIDE);
      else expect(face.x - (rail.x + rail.width), "no inset beside the rail").toBeLessThan(SIDE);
      await shot(app, `${locale}-games-picker`);
    });
  });
}
