// The chat's apps sheet (Ioan, 2026-10-02): Ionic's sheet modal with a segment for the tools and
// one for the games, as the apps themselves open. On a phone it is the width of the screen; on a
// tablet, beside the list, it covers exactly the chat pane. Since 2026-10-08 the apps are tiles, as
// in the Apps tab: a long name takes two lines at most, then it is cut.
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";

/** Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test e2e/apps-sheet.spec.ts`. */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(400);
  await app.screenshot({ path: `${dir}/${name}.png` });
}

/** The sheet's box, once Ionic has finished bringing it up. */
async function sheetBox(app: Page) {
  const wrapper = app.locator("ion-modal.ft-apps-sheet .modal-wrapper");
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

const SIZES = [
  { name: "phone", width: 360, height: 740, chat: `/chat/${BOB}`, apps: "[data-test='apps']" },
  // The tablets: the conversation sits beside the list (1280×800, and the Lenovo's 2560×1600).
  { name: "tablet-1280", width: 1280, height: 800, chat: "/tabs/chats", apps: ".ft-chats__detail [data-test='apps']" },
  { name: "tablet-1400", width: 1400, height: 875, chat: "/tabs/chats", apps: ".ft-chats__detail [data-test='apps']" },
];

for (const appearance of ["dark", "light"]) {
  for (const size of SIZES) {
    test.describe(`${size.name}, ${appearance}`, () => {
      test.use({ viewport: { width: size.width, height: size.height } });
      test.beforeEach(async ({ app }) => {
        await app.addInitScript((chosen) => localStorage.setItem("ft-appearance", chosen), appearance);
      });

      test("the sheet opens on the tools, lies on the screen and switches to the games", async ({ app }) => {
        await app.goto(size.chat);
        await app.locator(size.apps).click();
        await expect(app.getByTestId("apps-tab-tools")).toHaveClass(/segment-button-checked/);
        await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeVisible();
        // A dialog with a name, for a screen reader.
        await expect(app.getByRole("dialog", { name: "Apps" })).toBeVisible();
        // One sheet, even with another page of the app kept behind this one.
        await expect(app.locator("ion-modal.ft-apps-sheet")).toHaveCount(1);
        const box = await sheetBox(app);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(size.width + 0.5);
        if (size.width >= 768) {
          // The tablet (decided 2026-10-02): the sheet covers exactly the chat pane, beside the list.
          const pane = (await app.locator(".ft-chats__detail").boundingBox())!;
          expect(Math.abs(box.x - pane.x), "starts where the chat pane starts").toBeLessThanOrEqual(1);
          expect(Math.abs(box.x + box.width - size.width), "ends at the window's end").toBeLessThanOrEqual(1);
        }
        await shot(app, `${size.name}-plugins-${appearance}`);

        await app.getByTestId("apps-tab-games").click();
        await expect(app.getByTestId("games-sheet")).toContainText("Tic-tac-toe");
        await shot(app, `${size.name}-games-${appearance}`);
      });
    });
  }
}

// Android's back button closes the sheet and leaves the user in the chat (the app's own handler:
// Ionic only hears the back button through Capacitor or Cordova, not under Tauri).
test("the back button closes the sheet and stays in the chat", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeVisible();
  expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(true);
  await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeHidden();
  await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
  // With nothing open any more, the back button is the system's again.
  expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(false);
});

test.describe("with many plugins, on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });
  test("a long name takes two lines at most, then it is cut", async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 30));
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    const label = app.getByTestId("app-com.example.tool00").locator(".ft-app-tile__name");
    await expect(label).toBeVisible();
    // Two lines, then cut with an ellipsis; the tiles of a row keep one height.
    const cut = await label.evaluate((el) => {
      const line = parseFloat(getComputedStyle(el).lineHeight);
      return { cut: el.scrollHeight > el.clientHeight + 1, lines: Math.round(el.getBoundingClientRect().height / line) };
    });
    expect(cut).toEqual({ cut: true, lines: 2 });
    await shot(app, "phone-many-plugins-dark");
  });
});

// Device review of app#121 (iPhone): in Ionic's iOS look a segment's icon touched its label (2 px).
// Both looks leave the mockup's gap, on the Apps tab and in the chat's apps sheet; Android's is
// not doubled.
for (const mode of ["ios", "md"] as const) {
  test(`${mode}: a segment's icon keeps its gap to the label`, async ({ app }) => {
    const gaps = async (button: import("@playwright/test").Locator) =>
      button.evaluate((el) => {
        const icon = el.querySelector("ion-icon")!.getBoundingClientRect();
        const label = el.querySelector("ion-label")!.getBoundingClientRect();
        return Math.round(label.left - icon.right);
      });
    await app.goto(`/tabs/apps?ionic:mode=${mode}`);
    await expect(app.locator("html")).toHaveClass(new RegExp(`\\b${mode}\\b`));
    for (const id of ["tools", "games"]) {
      const gap = await gaps(app.getByTestId(`apps-segment-${id}`));
      expect(gap, `Apps tab, ${id}`).toBeGreaterThanOrEqual(6);
      expect(gap, `Apps tab, ${id}`).toBeLessThanOrEqual(10);
    }
    await app.goto(`/chat/${BOB}?ionic:mode=${mode}`);
    await app.getByTestId("apps").click();
    const gap = await gaps(app.getByTestId("apps-tab-tools"));
    expect(gap, "chat sheet").toBeGreaterThanOrEqual(6);
    expect(gap, "chat sheet").toBeLessThanOrEqual(10);
  });
}

// Second device review of app#121: on a 360 px phone "Backgammon" broke as "Backgammo/n". A word
// of ten letters keeps one line, two words wrap at the space, and a long name fits two lines.
test.describe("tile names on a 360 px phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("break between words, and never inside a word that fits", async ({ app }) => {
    const game = (id: string, name: string) => ({ id, name, version: "1.0.0", summary: "A game.", size: 40_000, carried: false, kind: "game" });
    await app.addInitScript(
      (entries) => ((window as unknown as Record<string, unknown>).__ftFakeCatalogue = entries),
      [game("com.flickertalk.game.backgammon", "Backgammon"), game("com.flickertalk.game.seabattle", "Batalla naval"), game("com.flickertalk.game.wordgrid", "Cuadrícula de letras")],
    );
    await app.goto("/tabs/apps?show=games");
    /** How many lines the name takes, whether the clamp cuts it, and the words split over two lines. */
    const lines = (id: string) =>
      app.getByTestId(`install-${id}`).locator(".ft-app-tile__name").evaluate((el) => {
        const node = el.firstChild as Text;
        const tops = (from: number, to: number) => {
          const range = document.createRange();
          range.setStart(node, from);
          range.setEnd(node, to);
          return new Set([...range.getClientRects()].filter((rect) => rect.width > 0).map((rect) => Math.round(rect.top))).size;
        };
        const text = node.textContent ?? "";
        const split: string[] = [];
        let at = 0;
        for (const word of text.trim().split(/\s+/)) {
          const start = text.indexOf(word, at);
          if (tops(start, start + word.length) > 1) split.push(word);
          at = start + word.length;
        }
        return { lines: tops(0, text.length), cut: el.scrollHeight > el.clientHeight + 1, split };
      });
    expect(await lines("com.flickertalk.game.backgammon")).toEqual({ lines: 1, cut: false, split: [] });
    // On one line or two, but never "na-/val": a wrap falls at the space.
    const two = await lines("com.flickertalk.game.seabattle");
    expect(two.lines).toBeLessThanOrEqual(2);
    expect(two).toMatchObject({ cut: false, split: [] });
    const long = await lines("com.flickertalk.game.wordgrid");
    expect(long.lines).toBeLessThanOrEqual(2);
    expect(long).toMatchObject({ cut: false, split: [] });
  });
});
