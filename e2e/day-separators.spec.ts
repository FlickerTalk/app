import { test as base, expect, type Page } from "@playwright/test";
import { installFakeCore } from "./fake-core";

const test = base.extend<{ app: Page }>({
  app: async ({ page }, use) => {
    await page.clock.setFixedTime(new Date("2026-10-02T00:30:00+02:00"));
    await page.addInitScript(installFakeCore);
    await use(page);
  },
});

const BOB = "ft_bob123456789";

test.use({ timezoneId: "Europe/Madrid" });

async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (dir) await app.screenshot({ path: `${dir}/${name}.png` });
}

async function openThread(app: Page) {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeLongChat = 40));
  await app.goto(`/chat/${BOB}`);
  await expect(app.locator(".ft-bubble").first()).toBeVisible();
}

test.describe("in English", () => {
  test.use({ locale: "en" });

  test("names the evening before by its date and the rest as today", async ({ app }) => {
    await openThread(app);
    await expect(app.locator(".ft-thread__day")).toHaveText(["October 1", "Today"]);
    await app.locator(".ft-thread__day").last().scrollIntoViewIfNeeded();
    await shot(app, "day-separators-en");
  });
});

test.describe("in Spanish", () => {
  test.use({ locale: "es" });

  test("writes the date in the language of the app", async ({ app }) => {
    await openThread(app);
    await expect(app.locator(".ft-thread__day")).toHaveText(["1 de octubre", "Hoy"]);
  });
});

test.describe("in Arabic", () => {
  test.use({ locale: "ar" });

  test("keeps the separators, right to left", async ({ app }) => {
    await openThread(app);
    await expect(app.locator("html")).toHaveAttribute("dir", "rtl");
    const days = app.locator(".ft-thread__day");
    await expect(days).toHaveCount(2);
    await expect(days.last()).toHaveText("اليوم");
    await days.last().scrollIntoViewIfNeeded();
    await shot(app, "day-separators-ar");
  });
});
