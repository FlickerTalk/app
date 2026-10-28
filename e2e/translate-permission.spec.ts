// 2026-10-20 (app#177): the phone's own translator is a permission like the others. Installing
// grants nothing (§53), so the Translator asks for it on the spot, in Ionic's sheet, the first
// time it asks the phone anything; Continue grants it and the tool gets the real answer, Cancel gives
// it each question's empty answer and grants nothing.
import type { Page } from "@playwright/test";
import { callsTo, expect, frameHeard, servePluginFrames, test } from "./helpers";

const TRANSLATOR = "com.flickertalk.translate";

test.beforeEach(async ({ app }) => {
  await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeTranslateTool = true));
  await servePluginFrames(app);
});

async function openTranslator(app: Page) {
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId(`app-${TRANSLATOR}`).click();
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
const translating = async (app: Page) => (await callsTo(app)).filter(([command]) => command.startsWith("core_translate_"));

test("a tool asks for the translator it lacks, and translates on the phone once allowed", async ({ app }) => {
  await openTranslator(app);
  await frameAsksLater(app, { type: "ft.translateText", id: "t1", text: "hello", from: "en", to: "es" });

  const ask = app.getByTestId("permission-ask");
  await expect(ask).toBeVisible();
  await expect(ask).toContainText("Translator needs a permission");
  await expect(app.getByTestId("permission-asked")).toContainText("Translate text on this phone");
  // Held: the plugin waits and the phone is not asked yet.
  expect(await answerTo(app, "t1")).toBeUndefined();
  expect(await translating(app)).toHaveLength(0);

  await app.getByTestId("permission-allow").click();
  await expect.poll(() => answerTo(app, "t1")).toEqual({ answer: "hola" });
  await expect(ask).toHaveCount(0);
  const [grant] = await commands(app, "core_plugin_grant");
  expect(grant?.[1]).toMatchObject({ plugin: TRANSLATOR, granted: { translate: true, messages: false, send: "nothing" } });

  // Granted now: the rest of the flow asks nothing more.
  await frameAsksLater(app, { type: "ft.translateLanguages", id: "t2" });
  await expect.poll(() => answerTo(app, "t2")).toEqual({ answer: ["en", "es"] });
  await frameAsksLater(app, { type: "ft.translateStatus", id: "t3", from: "en", to: "es" });
  await expect.poll(() => answerTo(app, "t3")).toEqual({ answer: "installed" });
  await expect(ask).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(1);
});

test("a tool told no gets the empty answers, and nothing is granted", async ({ app }) => {
  await openTranslator(app);
  await frameAsksLater(app, { type: "ft.translateLanguages", id: "t1" });
  await app.getByTestId("permission-cancel").click();
  await expect.poll(() => answerTo(app, "t1")).toEqual({ answer: [] });
  // Asked again for the next question, once the first sheet is gone: a no is not remembered.
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  await frameAsksLater(app, { type: "ft.translateText", id: "t2", text: "hello", from: "en", to: "es" });
  await expect(app.getByTestId("permission-ask")).toBeVisible();
  await app.getByTestId("permission-cancel").click();
  await expect.poll(() => answerTo(app, "t2")).toEqual({ answer: null });
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  expect(await commands(app, "core_plugin_grant")).toHaveLength(0);
  expect(await translating(app)).toHaveLength(0);
});
