import { expect, test } from "./helpers";

// A real FlickerTalk ID: "ft_" and the base58 of a 32-byte hash, 47 characters with no spaces.
const ID = "ft_9xQz4KpW2mRtV7bNcJ8eYh3LsDf6GaU1iOkXwZq5Tr2E";

// The narrowest iPhone the app runs on (SE, 13 mini at 375): the ID seen in the App Store
// screenshots ran off both sides of the screen (2026-09-29).
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
  // Nothing inside it is cut off or scrolls sideways either.
  expect(await shown.evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);
});
