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
import { expect, servePluginFrames, test } from "./helpers";

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
async function settled(one: Locator, label = "it"): Promise<Box> {
  let last = "";
  await expect
    .poll(async () => {
      const now = JSON.stringify(await one.boundingBox());
      const still = now === last && now !== "null";
      last = now;
      return still;
    }, { intervals: [100], message: `${label} stands still on the screen` })
    .toBe(true);
  return (await one.boundingBox())!;
}

/** Every visible match stands between the two side insets. */
async function clearOfTheSides(what: Locator, label: string) {
  await settled(what.filter({ visible: true }).first(), label);
  const width = what.page().viewportSize()!.width;
  const boxes = (await what.evaluateAll((all) =>
    all
      .map((one) => one.getBoundingClientRect())
      .filter((box) => box.width > 0 && box.height > 0)
      .map((box) => ({ x: box.x, right: box.right })),
  )) as Array<{ x: number; right: number }>;
  expect(boxes.length, `${label}: something to measure`).toBeGreaterThan(0);
  for (const box of boxes) {
    expect(box.x, `${label} starts under the left inset`).toBeGreaterThanOrEqual(SIDE - 0.5);
    expect(box.right, `${label} ends under the right inset`).toBeLessThanOrEqual(width - SIDE + 0.5);
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

      // Nor does the list, between the rail and the chat: its toolbar's buttons end where Ionic
      // ends them, not an inset away from the chat pane.
      const list = (await app.locator(".ft-chats__list").boundingBox())!;
      const buttons = (await app.locator(".ft-chats__head ion-buttons").first().boundingBox())!;
      if (locale === "ar") expect(buttons.x - list.x, "no inset beside the chat pane").toBeLessThan(SIDE);
      else expect(list.x + list.width - (buttons.x + buttons.width), "no inset beside the chat pane").toBeLessThan(SIDE);

      await pane.locator("[data-test='apps']").click();
      const sheet = app.locator("ion-modal.ft-apps-sheet");
      await clearOfTheSides(sheet.locator(".ft-app-tile__icon"), "an apps sheet icon");
      await clearOfTheSides(sheet.locator(".ft-app-tile__name"), "an apps sheet name");
      // Same pane, same rule: the grid of tiles starts where the content does, beside the list.
      const grid = (await sheet.locator(".ft-app-grid").first().boundingBox())!;
      if (locale === "ar") expect(paneBox.x + paneBox.width - (grid.x + grid.width), "no inset beside the list").toBeLessThan(SIDE);
      else expect(grid.x - paneBox.x, "no inset beside the list").toBeLessThan(SIDE);
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
      await clearOfTheSides(sheet.locator(".ft-app-tile__icon"), "an apps sheet icon");
      await clearOfTheSides(sheet.locator(".ft-app-tile__name"), "an apps sheet name");
      await shot(app, `${locale}-alone-apps-sheet`);

      await app.getByTestId("app-com.flickertalk.sketch").click();
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await clearOfTheSides(app.getByTestId("close-app"), "the tool's way out");
      await clearOfTheSides(app.locator("iframe.ft-plugin__frame"), "the tool");
      // Ionic's bar keeps clear of the sides by itself (2026-10-08): the window must not add the
      // inset a second time, or the way out sits far from the edge.
      const out = await settled(app.getByTestId("close-app"), "the tool's way out");
      const edge = Math.min(out.x, WIDTH - (out.x + out.width));
      expect(edge, "the tool's way out keeps the inset once").toBeLessThan(SIDE + 24);
      await shot(app, `${locale}-alone-tool`);
    });

    test("the Apps tab's games: the permissions sheet and the contact picker stay clear of the sides", async ({ app }) => {
      await app.goto("/tabs/apps?show=games");
      await app.getByTestId(`app-${TICTACTOE}`).click();
      await expect(app.getByTestId("game-allow")).toBeVisible();
      await clearOfTheSides(app.locator("[data-test='game-permissions'] > *"), "the permissions sheet");
      await shot(app, `${locale}-games-permissions`);

      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
      await clearOfTheSides(app.locator("ion-modal.ft-apps__picker ion-item ion-label"), "a contact's name");
      const picture = app.locator("ion-modal.ft-apps__picker ion-item [slot='start']");
      await clearOfTheSides(picture, "a contact's picture");
      // The sheet starts past the rail, away from the edge: the pictures start where Ionic puts them.
      const rail = (await app.locator("nav.ft-rail").boundingBox())!;
      const face = (await picture.first().boundingBox())!;
      if (locale === "ar") expect(rail.x - (face.x + face.width), "no inset beside the rail").toBeLessThan(SIDE);
      else expect(face.x - (rail.x + rail.width), "no inset beside the rail").toBeLessThan(SIDE);
      await shot(app, `${locale}-games-picker`);
    });

    test("the pages of the tabs and of Settings keep their texts and buttons clear of the sides", async ({ app }) => {
      const PAGES: Array<[string, string[]]> = [
        ["/tabs/calls", ["[data-test='empty']"]],
        ["/tabs/apps", [".ft-app-tile__name", ".ft-app-tile__icon", "[data-test='apps-more-title']"]],
        ["/tabs/settings", [".ft-me", ".ft-me button", "ion-item ion-label", "[data-test='premium-head']", "[data-test='premium-note']"]],
        ["/blocked", ["[data-test='empty']"]],
      ];
      for (const [route, parts] of PAGES) {
        await app.goto(route);
        for (const part of parts) await clearOfTheSides(app.locator(`.ion-page:not(.ion-page-hidden) ${part}`), `${route} ${part}`);
        await shot(app, `${locale}-page-${route.replaceAll("/", "-")}`);
      }
    });

    test("a tool on its own page stays clear of the sides", async ({ app }) => {
      await app.goto("/plugin/com.flickertalk.sketch");
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await clearOfTheSides(app.locator("iframe.ft-plugin__frame"), "the tool");
      await shot(app, `${locale}-plugin-page`);
    });

    test("a circle: its note, bubbles and composer stay clear of the sides", async ({ app }) => {
      await app.goto("/circle/circle1");
      await expect(app.getByTestId("circle-send")).toBeVisible();
      await clearOfTheSides(app.locator(".ft-thread__note"), "the circle's note");
      await clearOfTheSides(app.locator(".ft-bubble"), "a bubble");
      await clearOfTheSides(app.locator(".ft-composer__row"), "the composer");
      await shot(app, `${locale}-circle`);
    });

    test("a request: its panel stays clear of the side it reaches", async ({ app }) => {
      await app.goto("/tabs/chats");
      await app.getByTestId("requests").getByTestId("request-row").click();
      await expect(app.getByTestId("request-panel")).toBeVisible();
      await clearOfTheSides(app.locator("[data-test='request-panel'] p, [data-test='request-panel'] button"), "the request panel");
      await shot(app, `${locale}-request`);
    });

    test("the game room: the game, the last line and the composer stay clear of the sides", async ({ app }) => {
      await servePluginFrames(app);
      await app.goto(`/chat/${BOB}`);
      await app.getByTestId("apps").click();
      await app.getByTestId("apps-tab-games").click();
      await app.getByTestId(`game-${TICTACTOE}`).click();
      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId("game-room")).toBeVisible();
      await clearOfTheSides(app.locator("[data-test='game-area'] iframe"), "the game");
      await clearOfTheSides(app.locator("[data-test='game-bar'] ion-buttons"), "the game's bar");
      await clearOfTheSides(app.locator("[data-test='game-strip'] ion-icon, [data-test='game-strip'] ion-label"), "the last line");
      await clearOfTheSides(app.locator(".ft-composer__row"), "the composer");
      await shot(app, `${locale}-game-room`);
    });

    test("the message actions stay clear of the sides, however long the bar grows", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await app.locator(".ft-bubble").first().hover();
      await app.mouse.down();
      await app.waitForTimeout(700);
      await app.mouse.up();
      await expect(app.getByTestId("actions")).toBeVisible();
      await app.getByTestId("forward").click();
      // A bar of many contacts wraps at the room it is given: the test gives it all of it.
      await app.locator(".ft-actions__bar").evaluate((bar) => ((bar as HTMLElement).style.width = "100%"));
      await clearOfTheSides(app.locator(".ft-actions__bar"), "the actions bar");
      await shot(app, `${locale}-actions`);
    });
  });
}

// One column (narrower than the split view, 740×360: a small Android phone held sideways): no rail,
// the tab bar at the bottom, every page from edge to edge.
for (const locale of ["en", "ar"]) {
  test.describe(`one column, ${locale}`, () => {
    test.use({ viewport: { width: 740, height: 360 }, locale });

    test("the list, Settings and a conversation stay clear of both sides", async ({ app }) => {
      await app.goto("/tabs/chats");
      await expect(app.getByTestId("requests")).toBeVisible();
      await clearOfTheSides(app.locator(".ft-requests__title, .ft-requests__hint, [data-test='request-row']"), "a request");
      await clearOfTheSides(app.locator(".ft-row"), "a chat's row");
      await clearOfTheSides(app.locator("ion-tab-bar ion-tab-button"), "a tab");
      await shot(app, `${locale}-narrow-chats`);

      await app.goto("/tabs/settings");
      await clearOfTheSides(app.locator(".ion-page:not(.ion-page-hidden) .ft-me"), "the identity card");
      // Clear once, not twice: an item keeps Ionic's own padding past the inset.
      const icon = (await app.locator(".ion-page:not(.ion-page-hidden) ion-item ion-icon").first().boundingBox())!;
      if (locale === "ar") expect(740 - SIDE - (icon.x + icon.width), "one inset, not two").toBeLessThan(SIDE);
      else expect(icon.x - SIDE, "one inset, not two").toBeLessThan(SIDE);
      await shot(app, `${locale}-narrow-settings`);

      await app.goto(`/chat/${BOB}`);
      await clearOfTheSides(app.locator(".ft-bubble"), "a bubble");
      await clearOfTheSides(app.locator(".ft-composer__row"), "the composer");
    });
  });
}
