// The chat's apps sheet (Ioan, 2026-10-03): Ionic's sheet modal with a segment for the plugins and
// one for the games, as the apps themselves open. On a phone it is the width of the screen; on a
// tablet, beside the list, Ionic centres it with a limited width. Long names are cut, not wrapped.
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

      test("the sheet opens on the plugins, lies on the screen and switches to the games", async ({ app }) => {
        await app.goto(size.chat);
        await app.locator(size.apps).click();
        await expect(app.getByTestId("apps-tab-tools")).toHaveClass(/segment-button-checked/);
        await expect(app.getByTestId("app-com.flickertalk.markdown")).toBeVisible();
        // A dialog with a name, for a screen reader.
        await expect(app.getByRole("dialog", { name: "Plugins" })).toBeVisible();
        // One sheet, even with another page of the app kept behind this one.
        await expect(app.locator("ion-modal.ft-apps-sheet")).toHaveCount(1);
        const box = await sheetBox(app);
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(size.width + 0.5);
        if (size.width >= 768) {
          // Centred, and not the width of the whole tablet.
          expect(box.width).toBeLessThanOrEqual(600);
          expect(Math.abs(box.x + box.width / 2 - size.width / 2)).toBeLessThan(2);
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
  test("a long name is cut, not wrapped", async ({ app }) => {
    await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 30));
    await app.goto(`/chat/${BOB}`);
    await app.getByTestId("apps").click();
    const label = app.getByTestId("app-com.example.tool00").locator("ion-label");
    await expect(label).toBeVisible();
    // One line, cut with an ellipsis.
    const cut = await label.evaluate((el) => ({ cut: el.scrollWidth > el.clientWidth, oneLine: el.getBoundingClientRect().height < 1.6 * parseFloat(getComputedStyle(el).fontSize) }));
    expect(cut).toEqual({ cut: true, oneLine: true });
    await shot(app, "phone-many-plugins-dark");
  });
});
