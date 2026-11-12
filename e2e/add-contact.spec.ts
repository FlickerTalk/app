import { expect, test } from "./helpers";

const ID = "ft_9xQz4KpW2mRtV7bNcJ8eYh3LsDf6GaU1iOkXwZq5Tr2E";

test.use({ viewport: { width: 320, height: 640 } });

test("the device ID fits the add-contact screen, whole", async ({ app }) => {
  await app.addInitScript((id) => {
    (window as unknown as Record<string, unknown>).__ftFakeMeId = id;
  }, ID);
  await app.goto("/add-contact");

  const shown = app.locator(".ft-add__id");
  await expect(shown).toHaveText(ID);
  const box = await shown.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(320);
  expect(await shown.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
});
