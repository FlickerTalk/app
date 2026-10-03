// Android's back button on a plugin opened on its own (issue #61, seen on the Samsung, 2026-10-03):
// Back left the app from Settings → Plugins → a tool, and the app came back on the tool. The
// system's Back follows the WebView's history, and Android's WebView skips the entries that were
// added without a touch on the page (a reminder tapped in the notifications opens this page that
// way): with nothing it may go back to, it hands Back to Android, which puts the app away. The page
// takes the button itself, and goes where its own back arrow goes.
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const MARKDOWN = "com.flickertalk.markdown";

/** Android's back button: the app's own handling if it listens, the system's (the WebView's history) if not. */
async function pressBack(app: Page): Promise<"app" | "system"> {
  const heard = await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());
  if (heard) return "app";
  await app.goBack();
  return "system";
}

/** The page has taken the button (registering it with the core is not instant). */
async function listening(app: Page) {
  await expect.poll(() => app.evaluate(() => (window as unknown as { __ftFake: { listening: () => boolean } }).__ftFake.listening())).toBe(true);
}

test.describe("on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("Back on a tool opened from Settings → Plugins goes back to Plugins, by the app itself", async ({ app }) => {
    await app.goto("/tabs/settings");
    await app.getByTestId("plugins").click();
    await expect(app).toHaveURL(/\/plugins$/);
    await app.getByTestId(`open-${MARKDOWN}`).click();
    await expect(app).toHaveURL(new RegExp(`/plugin/${MARKDOWN}$`));
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/plugins$/);
  });

  test("with nothing behind it, Back on a tool goes to Settings, as its back arrow does", async ({ app }) => {
    await app.goto(`/plugin/${MARKDOWN}`);
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/tabs\/settings$/);
  });

  test("once the tool's page is left, the app lets go of Back", async ({ app }) => {
    await app.goto("/tabs/settings");
    await app.getByTestId("plugins").click();
    await app.getByTestId(`open-${MARKDOWN}`).click();
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/plugins$/);
    await expect.poll(() => app.evaluate(() => (window as unknown as { __ftFake: { listening: () => boolean } }).__ftFake.listening())).toBe(false);
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/tabs\/settings$/);
  });
});
