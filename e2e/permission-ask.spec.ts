// Ioan, 2026-10-08: «si no tiene permiso se debería pedir siempre». Installing grants nothing
// (§53), so Location, opened with its permission off, used to answer «Location not available» and
// nothing offered to turn it on. Now the app asks for that one permission on the spot, in Ionic's
// sheet; Allow grants it and the tool gets the real answer, Cancel gives it the refusal as before.
import type { Page } from "@playwright/test";
import { callsTo, expect, frameHeard, holdTile, servePluginFrames, test } from "./helpers";

const LOCATION = "com.flickertalk.location";
const FIX = { lat: 40.41678, lon: -3.70379, accuracy: 35, at: 1_790_000_000_000 };

test.beforeEach(async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeLocationTool = true));
  await servePluginFrames(app);
});

async function openLocation(app: Page) {
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId(`app-${LOCATION}`).click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
}

/** The plugin asks something and does not wait here: its answer comes once the user has said. */
async function frameAsksLater(app: Page, message: Record<string, unknown>) {
  await expect.poll(() => app.frames().some((frame) => frame.url().startsWith("http://ftplugin.localhost/"))).toBe(true);
  const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  await expect.poll(() => frame.evaluate(() => (window as unknown as { heard?: unknown[] }).heard !== undefined)).toBe(true);
  await frame.evaluate((asked) => parent.postMessage(asked, "*"), message);
}

/** What the plugin was finally answered to the question `id`; `undefined` while it waits. */
async function answerTo(app: Page, id: string) {
  const done = (await frameHeard(app)).find((one) => (one as { type?: string; id?: string }).type === "ft.done" && (one as { id?: string }).id === id);
  return done ? { answer: (done as { answer: unknown }).answer } : undefined;
}

const commands = async (app: Page, name: string) => (await callsTo(app)).filter(([command]) => command === name);

test("a tool asks for the location it lacks, and gets the position once allowed", async ({ app }) => {
  await openLocation(app);
  await frameAsksLater(app, { type: "ft.location", id: "l1" });

  const ask = app.getByTestId("permission-ask");
  await expect(ask).toBeVisible();
  await expect(ask).toContainText("Location needs a permission");
  await expect(app.getByTestId("permission-asked")).toContainText("Your location, only when you ask");
  // Held: the plugin waits and the core is not asked yet.
  expect(await answerTo(app, "l1")).toBeUndefined();
  expect(await commands(app, "core_plugin_location")).toHaveLength(0);

  await app.getByTestId("permission-allow").click();
  await expect.poll(() => answerTo(app, "l1")).toEqual({ answer: FIX });
  await expect(ask).toHaveCount(0);
  const [grant] = await commands(app, "core_plugin_grant");
  expect(grant?.[1]).toMatchObject({ plugin: LOCATION, granted: { location: true, send: "nothing", live: false } });
  // The tool is still open, and the next time nothing is asked.
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  await frameAsksLater(app, { type: "ft.location", id: "l2" });
  await expect.poll(() => answerTo(app, "l2")).toEqual({ answer: FIX });
  await expect(ask).toHaveCount(0);

  // The switch in the Apps tab says it is on (got there inside the app: a reload is a new core).
  await app.getByTestId("close-app").click();
  await app.locator(".ft-thread__bar ion-back-button").last().click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.getByRole("tab", { name: "Apps" }).click();
  await expect(app).toHaveURL(/\/tabs\/apps/);
  await holdTile(app, `app-${LOCATION}`);
  await expect(app.getByTestId("toggle-location")).toHaveJSProperty("checked", true);
  await expect(app.getByTestId("toggle-send")).toHaveJSProperty("checked", false);
});

test("a tool told no gets no position, and nothing is granted", async ({ app }) => {
  await openLocation(app);
  await frameAsksLater(app, { type: "ft.location", id: "l1" });
  await app.getByTestId("permission-cancel").click();
  await expect.poll(() => answerTo(app, "l1")).toEqual({ answer: null });
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(0);
  expect(await commands(app, "core_plugin_location")).toHaveLength(0);
});

test("a tool asks to write in the chat, and what it made waits in the composer once allowed", async ({ app }) => {
  await openLocation(app);
  await frameAsksLater(app, { type: "ft.made", name: "here.txt", mime: "text/plain", data: "aGVyZQ==" });
  await expect(app.getByTestId("permission-asked")).toContainText("Write in the chat");
  await app.getByTestId("permission-allow").click();
  await expect(app.getByTestId("staged")).toContainText("here.txt");
  const [grant] = await commands(app, "core_plugin_grant");
  expect(grant?.[1]).toMatchObject({ plugin: LOCATION, granted: { send: "propose", location: false } });
});

test("a tool asks for the live channel, and talks to its twin once allowed", async ({ app }) => {
  await openLocation(app);
  await frameAsksLater(app, { type: "ft.liveSend", id: "q1", data: "AQID" });
  await expect(app.getByTestId("permission-asked")).toContainText("Talk to the same plugin on the other side of the chat");
  await app.getByTestId("permission-allow").click();
  await expect.poll(() => answerTo(app, "q1")).toEqual({ answer: true });
  expect((await commands(app, "core_plugin_live_send"))[0]?.[1]).toMatchObject({ plugin: LOCATION, contact: "ft_bob123456789", data: "AQID" });
});

test("a tool on its own page asks too", async ({ app }) => {
  await app.goto(`/plugin/${LOCATION}`);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  await frameAsksLater(app, { type: "ft.location", id: "l1" });
  await expect(app.getByTestId("permission-ask")).toContainText("Location needs a permission");
  await app.getByTestId("permission-allow").click();
  await expect.poll(() => answerTo(app, "l1")).toEqual({ answer: FIX });
});
