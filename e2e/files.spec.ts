// A4 of the 2026-09-24 review: a file bigger than the phone downloads on its own waits for a tap,
// says its size, and only then is asked for.
import { callsTo, expect, test } from "./helpers";

test("a big file waits for a tap and is then asked for", async ({ app }) => {
  await app.goto("/chat/ft_bob123456789");
  const download = app.getByTestId("download");
  await expect(download).toBeVisible();
  await expect(app.locator(".ft-bubble.is-file")).toContainText("Tap to download");
  await expect(app.locator(".ft-bubble.is-file")).toContainText("48 MB");
  expect((await callsTo(app)).some(([command]) => command === "core_accept_file")).toBe(false);

  await download.click();
  await expect.poll(async () => (await callsTo(app)).some(([command, args]) => command === "core_accept_file" && args?.message === "big")).toBe(true);
  // The core announced the first chunks: the bubble shows progress now, not the tap.
  await expect(app.getByTestId("download")).toHaveCount(0);
  await expect(app.locator("[role='progressbar']")).toHaveAttribute("aria-valuenow", "25");
});

test("the size files download on their own is chosen in settings", async ({ app }) => {
  await app.goto("/tabs/settings");
  const select = app.getByTestId("auto-download");
  await expect(select).toBeVisible();
  await expect(select).toHaveJSProperty("value", 10 * 1024 * 1024);
  await expect(select).toContainText("Up to 10 MB");
});
