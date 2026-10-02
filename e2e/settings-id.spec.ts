// The two buttons beside the ID in Settings (seen in the UI review, 2026-10-02, doing nothing): one
// copies the ID shown and says so for a moment; the other opens this phone's QR code.
import { expect, test } from "./helpers";

test.use({ viewport: { width: 360, height: 740 } });

test("copies the ID shown to the clipboard and says so", async ({ app, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await app.goto("/tabs/settings");
  const shown = await app.locator(".ft-me__id").textContent();
  const copy = app.getByTestId("copy-id");
  await expect(copy).toHaveAttribute("aria-label", "Copy ID");
  await copy.click();
  await expect(copy).toHaveAttribute("aria-label", "ID copied");
  expect(await app.evaluate(() => navigator.clipboard.readText())).toBe(shown);
  // A moment later it is the copy button again.
  await expect(copy).toHaveAttribute("aria-label", "Copy ID", { timeout: 4000 });
});

test("shows this phone's own QR code", async ({ app }) => {
  await app.goto("/tabs/settings");
  await app.getByTestId("show-qr").click();
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("mode-code")).toHaveAttribute("aria-pressed", "true");
  await expect(app.getByRole("img", { name: "QR code" })).toBeVisible();
});
