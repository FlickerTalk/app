import type { Locator, Page } from "@playwright/test";
import { expect, frameSays, test } from "./helpers";

const BOB = "ft_bob123456789";
const INSET = 48;

test.use({ viewport: { width: 360, height: 853 } });

test.beforeEach(async ({ app }) => {
  await app.addInitScript((inset) => {
    document.addEventListener("DOMContentLoaded", () => document.documentElement.style.setProperty("--ion-safe-area-bottom", `${inset}px`));
  }, INSET);
});

async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(300);
  await app.screenshot({ path: `${dir}/${name}-${process.env.FT_SHOTS_TAG ?? "now"}.png` });
}

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
  await aboveTheBar(app, app.locator("[data-test='apps-sheet-tools'] .ft-app-tile").last());

  await app.getByTestId("apps-tab-games").click();
  await expect(app.getByTestId("more-games-link")).toBeVisible();
  await shot(app, "games-tab");
  await aboveTheBar(app, app.getByTestId("more-games-link"));
});

test("the games' permissions sheet and contact picker stay above the navigation bar", async ({ app }) => {
  await app.goto("/tabs/apps?show=games");
  await app.getByTestId("app-com.flickertalk.game.tictactoe").click();
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

test.describe("with many plugins and games", () => {
  test.use({ viewport: { width: 360, height: 740 } });
  test.beforeEach(async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 30));
  });

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
    await expect(app.getByTestId("game-permissions")).toBeVisible();
  });
});

test.describe("a tool taller than the screen", () => {
  async function scrollTool(app: Page, to: "half" | "end") {
    await app.evaluate((where) => {
      const scroller = document.querySelector(".ft-app__body") ?? document.querySelector(".ft-app");
      if (scroller) scroller.scrollTop = where === "end" ? scroller.scrollHeight : scroller.scrollHeight / 2;
    }, to);
  }

  async function frameEnd(app: Page) {
    const frame = app.locator("iframe.ft-plugin__frame");
    const height = async () => Math.round((await frame.boundingBox())?.height ?? 0);
    await expect.poll(height).toBeGreaterThan(300);
    let before = -1;
    await expect.poll(async () => {
      const now = await height();
      const still = now === before;
      before = now;
      return still;
    }).toBe(true);
    expect(await height()).toBeLessThan(1400);
    return frame;
  }

  test("in a chat's window, scrolled to its end, ends above the bar", async ({ app }) => {
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    await app.getByTestId("app-com.flickertalk.markdown").click();
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
    await frameSays(app, { type: "ft.height", height: 1400 });
    const frame = await frameEnd(app);
    await scrollTool(app, "end");
    await aboveTheBar(app, frame);
  });

  test("in a chat's window, scrolled, stays below its bar and out of the status bar", async ({ app }) => {
    const TOP = 30;
    await app.addInitScript((inset) => {
      document.addEventListener("DOMContentLoaded", () => document.documentElement.style.setProperty("--ion-safe-area-top", `${inset}px`));
    }, TOP);
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    await app.getByTestId("app-com.flickertalk.markdown").click();
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
    await frameSays(app, { type: "ft.height", height: 1400 });
    await frameEnd(app);
    for (const to of ["half", "end"] as const) {
      await scrollTool(app, to);
      const bar = (await app.locator(".ft-app__bar").boundingBox())!;
      expect(Math.round(bar.y), `${to}: the bar starts under the status bar`).toBe(TOP);
      await expect
        .poll(
          () => app.evaluate(([top, under]) => [document.elementFromPoint(180, top / 2), document.elementFromPoint(180, under - 2)].map((one) => one?.closest("iframe, .ft-app__bar, .ft-app")?.classList[0] ?? "nothing"), [TOP, bar.y + bar.height] as const),
          { message: to },
        )
        .toEqual(["ft-app", "ft-app__bar"]);
    }
  });

  test("on its own page, scrolled to its end, ends above the bar", async ({ app }) => {
    await app.goto("/plugin/com.flickertalk.markdown");
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
    await frameSays(app, { type: "ft.height", height: 1400 });
    const frame = await frameEnd(app);
    await app.locator("ion-content").last().evaluate((content) => (content as HTMLIonContentElement).scrollToBottom(0));
    await aboveTheBar(app, frame);
  });
});

test("the Apps tab, scrolled to its end, ends above the bar", async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 12));
  await app.goto("/tabs/apps");
  const page = app.locator(".ion-page:not(.ion-page-hidden) ion-content.ft-apps");
  const last = page.locator("[data-test='apps-offline'], [data-test='apps-all-here'], [data-test='apps-more'] .ft-app-tile").last();
  await expect(last).toBeVisible();
  await page.evaluate((content) => (content as HTMLIonContentElement).scrollToBottom(0));
  await aboveTheBar(app, last);
});

test("an app's sheet is as tall as what it says, and its last button stays above the bar", async ({ app }) => {
  await app.goto("/tabs/apps?show=games");
  await app.getByTestId("app-com.flickertalk.game.tictactoe").click({ button: "right" });
  await app.getByTestId("sheet-remove").click();
  const confirm = app.getByTestId("remove-confirm");
  await expect(confirm).toBeVisible();
  await shot(app, "app-sheet");
  await aboveTheBar(app, confirm);
  const sheet = (await app.locator("ion-modal.ft-app-sheet .modal-wrapper").boundingBox())!;
  const body = (await app.getByTestId("app-sheet").boundingBox())!;
  expect(Math.abs(sheet.y + sheet.height - (body.y + body.height)), "the sheet ends with its content").toBeLessThanOrEqual(2);
  expect(sheet.y + sheet.height).toBeLessThanOrEqual(app.viewportSize()!.height + 1);
  expect(sheet.y, "and leaves the page above it").toBeGreaterThan(200);
});

test("an app's sheet darkens the page as much as the game's permissions sheet", async ({ app }) => {
  const darkness = async (sheet: string) => {
    const backdrop = app.locator(`ion-modal.${sheet} ion-backdrop`);
    let last = "";
    await expect
      .poll(async () => {
        const now = await backdrop.evaluate((el) => getComputedStyle(el).opacity);
        const still = now === last;
        last = now;
        return still;
      }, { intervals: [150] })
      .toBe(true);
    return Number(last);
  };
  await app.goto("/tabs/apps?show=games");
  await app.getByTestId("app-com.flickertalk.game.tictactoe").click({ button: "right" });
  const ofApp = await darkness("ft-app-sheet");
  const room = async (sheet: string) => {
    const top = (await app.locator(`ion-modal.${sheet} .modal-wrapper`).boundingBox())!.y;
    return (await app.locator(`ion-modal.${sheet} img`).first().boundingBox())!.y - top;
  };
  const appRoom = await room("ft-app-sheet");
  await app.getByTestId("sheet-play").click();
  await expect(app.getByTestId("game-permissions")).toBeVisible();
  const ofPermissions = await darkness("ft-game-ask");
  expect(ofPermissions).toBeGreaterThan(0.3);
  expect(ofApp).toBeCloseTo(ofPermissions, 2);
  expect(Math.abs((await room("ft-game-ask")) - appRoom), "the picture sits as far under the handle").toBeLessThanOrEqual(2);
});

test.describe("on a short screen", () => {
  test.use({ viewport: { width: 360, height: 380 } });

  test("an app's sheet scrolls inside rather than running off the screen", async ({ app }) => {
    await app.addInitScript(() => {
      const fake = window as unknown as Record<string, unknown>;
      fake.__ftFakeCatalogue = [];
    });
    await app.goto("/tabs/apps?show=games");
    await app.getByTestId("app-com.flickertalk.game.tictactoe").click({ button: "right" });
    await app.getByTestId("sheet-remove").click();
    const body = app.getByTestId("app-sheet");
    await expect(app.getByTestId("remove-confirm")).toBeAttached();
    const sheet = (await app.locator("ion-modal.ft-app-sheet .modal-wrapper").boundingBox())!;
    expect(sheet.y, "within the screen").toBeGreaterThanOrEqual(0);
    expect(sheet.y + sheet.height).toBeLessThanOrEqual(380 + 1);
    expect(await body.evaluate((el) => el.scrollHeight > el.clientHeight), "it scrolls").toBe(true);
    await body.evaluate((el) => el.scrollTo(0, el.scrollHeight));
    await aboveTheBar(app, app.getByTestId("remove-confirm"));
  });
});

