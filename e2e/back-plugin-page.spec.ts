import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const MARKDOWN = "com.flickertalk.markdown";

async function pressBack(app: Page): Promise<"app" | "system"> {
  const heard = await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());
  if (heard) return "app";
  await app.goBack();
  return "system";
}

async function listening(app: Page) {
  await expect.poll(() => app.evaluate(() => (window as unknown as { __ftFake: { listening: () => boolean } }).__ftFake.listening())).toBe(true);
}

test.describe("on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("Back on a tool opened from the Apps tab goes back to it, by the app itself", async ({ app }) => {
    await app.goto("/tabs/settings");
    await app.getByRole("tab", { name: "Apps" }).click();
    await expect(app).toHaveURL(/\/tabs\/apps$/);
    await app.getByTestId(`app-${MARKDOWN}`).click();
    await expect(app).toHaveURL(new RegExp(`/plugin/${MARKDOWN}$`));
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/tabs\/apps$/);
  });

  test("with nothing behind it, Back on a tool goes to the Apps tab, as its back arrow does", async ({ app }) => {
    await app.goto(`/plugin/${MARKDOWN}`);
    await expect(app.locator("ion-back-button:visible")).toHaveAttribute("default-href", "/tabs/apps");
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/tabs\/apps$/);
  });

  test("once the tool's page is left, the app lets go of Back", async ({ app }) => {
    await app.goto("/tabs/settings");
    await app.getByRole("tab", { name: "Apps" }).click();
    await app.getByTestId(`app-${MARKDOWN}`).click();
    await listening(app);
    expect(await pressBack(app)).toBe("app");
    await expect(app).toHaveURL(/\/tabs\/apps$/);
    await expect.poll(() => app.evaluate(() => (window as unknown as { __ftFake: { listening: () => boolean } }).__ftFake.listening())).toBe(false);
    expect(await pressBack(app)).toBe("system");
    await expect(app).toHaveURL(/\/tabs\/settings$/);
  });
});
