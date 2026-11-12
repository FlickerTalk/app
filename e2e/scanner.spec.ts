import { commandsSent, expect, test } from "./helpers";
import type { Page } from "@playwright/test";

async function camera(app: Page, before: string, answer = before, after = answer) {
  await app.addInitScript((states) => {
    (window as unknown as Record<string, unknown>).__ftFakeScanCamera = states;
  }, { before, answer, after });
}

async function tapScan(app: Page) {
  await app.goto("/add-contact");
  await app.getByTestId("mode-scan").click();
  await app.getByTestId("scan-now").click();
}

test("a fresh install asks for the camera, then the camera opens over the page", async ({ app }) => {
  await camera(app, "prompt", "granted");
  await tapScan(app);
  await expect(app.getByTestId("scanner-overlay")).toBeVisible();
  const sent = await commandsSent(app);
  const asked = sent.indexOf("plugin:barcode-scanner|request_permissions");
  expect(asked).toBeGreaterThanOrEqual(0);
  expect(sent.indexOf("plugin:barcode-scanner|scan")).toBeGreaterThan(asked);

  await app.evaluate(() => (window as unknown as { __ftFake: { scanned: (text: string) => void } }).__ftFake.scanned("https://flickertalk.com/add#card"));
  await expect(app.getByTestId("scanner-overlay")).toBeHidden();
  expect(await commandsSent(app)).toContain("core_add_contact");
});

test("Back closes the camera, and only the camera", async ({ app }) => {
  await camera(app, "granted");
  await tapScan(app);
  await expect(app.getByTestId("scanner-overlay")).toBeVisible();

  const heard = await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());
  expect(heard).toBe(true);
  await expect(app.getByTestId("scanner-overlay")).toBeHidden();
  expect(await commandsSent(app)).toContain("plugin:barcode-scanner|cancel");
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("scan-now")).toBeVisible();
  expect(await app.evaluate(() => document.documentElement.classList.contains("ft-scanning"))).toBe(false);

  expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(false);
});

test("a refused camera is said, and the link can still be pasted", async ({ app }) => {
  await camera(app, "prompt", "denied", "prompt-with-rationale");
  await tapScan(app);
  const notice = app.getByTestId("camera-refused");
  await expect(notice).toBeVisible();
  await expect(notice).toContainText("Camera access is off");
  await expect(app.getByTestId("camera-settings")).toHaveCount(0);
  await expect(app.getByTestId("scanner-overlay")).toHaveCount(0);
  expect(await commandsSent(app)).not.toContain("plugin:barcode-scanner|scan");

  await notice.getByLabel("Close").click();
  await expect(notice).toBeHidden();
  await app.getByTestId("paste").fill("https://flickertalk.com/add#card");
  await app.getByTestId("add").click();
  expect(await commandsSent(app)).toContain("core_add_contact");
});

test("a camera refused for good points to the system settings", async ({ app }) => {
  await camera(app, "denied");
  await tapScan(app);
  await expect(app.getByTestId("camera-refused")).toContainText("Settings");
  const close = await app.getByTestId("camera-refused").getByLabel("Close").boundingBox();
  const settings = await app.getByTestId("camera-settings").boundingBox();
  expect(close!.y + close!.height).toBeLessThanOrEqual(settings!.y);
  expect(close!.x).toBeGreaterThan(settings!.x + settings!.width);
  await app.getByTestId("camera-settings").click();
  expect(await commandsSent(app)).toContain("plugin:barcode-scanner|open_app_settings");
  await expect(app.getByTestId("camera-refused")).toBeHidden();
});
