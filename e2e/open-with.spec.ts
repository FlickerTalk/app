import type { Page } from "@playwright/test";
import { callsTo, expect, frameHeard, holdTile, servePluginFrames, test } from "./helpers";

const REMINDERS = "com.flickertalk.remind";

test.beforeEach(async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeRemindTool = true));
  await servePluginFrames(app);
});

async function openWith(app: Page) {
  await app.locator("[data-message='m1'] [data-test='bubble']").hover();
  await app.mouse.down();
  await app.waitForTimeout(700);
  await app.mouse.up();
  await expect(app.getByTestId("actions")).toBeVisible();
  await app.getByTestId("open-with").click();
}

async function frameAsksLater(app: Page, message: Record<string, unknown>) {
  await expect.poll(() => app.frames().some((frame) => frame.url().startsWith("http://ftplugin.localhost/"))).toBe(true);
  const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  await expect.poll(() => frame.evaluate(() => (window as unknown as { heard?: unknown[] }).heard !== undefined)).toBe(true);
  await frame.evaluate((asked) => parent.postMessage(asked, "*"), message);
}

async function answerTo(app: Page, id: string) {
  const done = (await frameHeard(app)).find((one) => (one as { type?: string; id?: string }).type === "ft.done" && (one as { id?: string }).id === id);
  return done ? { answer: (done as { answer: unknown }).answer } : undefined;
}

async function openedWith(app: Page) {
  const open = (await frameHeard(app)).find((one) => (one as { type?: string }).type === "ft.open") as { text?: string; ref?: string } | undefined;
  return open && { text: open.text, ref: open.ref };
}

const commands = async (app: Page, name: string) => (await callsTo(app)).filter(([command]) => command === name);

test("a tool not yet allowed to read the message is offered, asks on the spot, and opens with it", async ({ app }) => {
  await app.goto("/chat/ft_bob123456789");
  await openWith(app);
  await expect(app.locator("[data-test^='open-with-']")).toHaveCount(1);
  await app.getByTestId(`open-with-${REMINDERS}`).click();

  const ask = app.getByTestId("permission-ask");
  await expect(ask).toBeVisible();
  await expect(ask).toContainText("Reminders needs a permission");
  await expect(app.getByTestId("permission-asked")).toContainText("Read what you send it");
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(0);

  await app.getByTestId("permission-allow").click();
  await expect(ask).toHaveCount(0);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  await expect.poll(() => openedWith(app)).toEqual({ text: "see you at six", ref: "ref_m1" });
  const [grant] = await commands(app, "core_plugin_grant");
  expect(grant?.[1]).toMatchObject({ plugin: REMINDERS, granted: { messages: true, remind: false } });

  await frameAsksLater(app, { type: "ft.remindSet", id: "r1", reminder: "r1", at: Date.now() + 3_600_000, text: "see you at six" });
  await expect(ask).toBeVisible();
  await expect(ask).toContainText("Reminders needs a permission");
  await expect(app.getByTestId("permission-asked")).toContainText("Set reminders on this phone");
  expect(await answerTo(app, "r1")).toBeUndefined();
  await app.getByTestId("permission-allow").click();
  await expect.poll(() => answerTo(app, "r1")).toEqual({ answer: true });
  expect((await commands(app, "core_remind_set"))[0]?.[1]).toMatchObject({ plugin: REMINDERS, id: "r1", text: "see you at six" });
  expect((await commands(app, "core_plugin_grant"))[1]?.[1]).toMatchObject({ plugin: REMINDERS, granted: { messages: true, remind: true } });

  await app.getByTestId("close-app").click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  await openWith(app);
  await app.getByTestId(`open-with-${REMINDERS}`).click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  await expect.poll(() => openedWith(app)).toEqual({ text: "see you at six", ref: "ref_m1" });
  await expect(ask).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(2);

  await app.getByTestId("close-app").click();
  await app.locator(".ft-thread__bar ion-back-button").last().click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.getByRole("tab", { name: "Apps" }).click();
  await expect(app).toHaveURL(/\/tabs\/apps/);
  await holdTile(app, `app-${REMINDERS}`);
  await expect(app.getByTestId("toggle-messages")).toHaveJSProperty("checked", true);
  await expect(app.getByTestId("toggle-remind")).toHaveJSProperty("checked", true);
});

test("told no, the tool opens nothing and is granted nothing, and is asked again the next time", async ({ app }) => {
  await app.goto("/chat/ft_bob123456789");
  await openWith(app);
  await app.getByTestId(`open-with-${REMINDERS}`).click();
  await app.getByTestId("permission-cancel").click();
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(0);
  expect(await commands(app, "core_plugin_ref")).toHaveLength(0);

  await openWith(app);
  await app.getByTestId(`open-with-${REMINDERS}`).click();
  await expect(app.getByTestId("permission-ask")).toBeVisible();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
});
