import type { Page } from "@playwright/test";
import { test } from "@playwright/test";
import { installFakeCore } from "./fake-core";
import { callsTo, expect, serveRealPluginFrames } from "./helpers";

const TRANSLATOR = "com.flickertalk.translate";
const CIRCLE = "circle1";
const SAID = "dinner on friday?";
const TRANSLATED = "¿cenamos el viernes?";

const plugin = (downloaded: boolean) => `
customElements.define("ft-probe", class extends HTMLElement {});
let downloaded = ${downloaded};
let set = { lang: "fr", receive: false, send: false };
const WORDS = { ${JSON.stringify(SAID)}: ${JSON.stringify(TRANSLATED)}, "hello": "bonjour" };
ft.onHook(async (hook) => {
  if (hook.name === "message") {
    if (!downloaded && !hook.answer) return hook.manual ? { ask: { text: "Download French? About 30 MB, once." } } : null;
    downloaded = true;
    return { text: WORDS[hook.text] ?? "" };
  }
  if (hook.name === "composer") return { text: WORDS[hook.text] ?? hook.text };
  if (hook.name === "settings") {
    if (hook.manual) set = { ...set, ...hook.values };
    const options = { lang: [{ value: "", label: "None" }, { value: "fr", label: "French (detected)" }] };
    return { values: set, options, auto: { message: set.receive, composer: set.send }, lang: set.lang || null };
  }
  return null;
});
`;

async function circle(app: Page, options: { downloaded?: boolean; flags?: Record<string, unknown>[] } = {}) {
  await app.addInitScript(
    ({ flags, circle }) => {
      const fake = window as unknown as Record<string, unknown>;
      fake.__ftFakeTranslateTool = true;
      fake.__ftFakeTranslateReads = true;
      if (flags) fake.__ftFakeChatHooks = { [circle]: flags };
    },
    { flags: options.flags, circle: CIRCLE },
  );
  await app.addInitScript(installFakeCore);
  await serveRealPluginFrames(app, plugin(options.downloaded ?? false));
  await app.goto(`/circle/${CIRCLE}`);
}

const bobsBubble = (app: Page) => app.locator("[data-message='cm1'] [data-test='bubble']");
const notesKept = async (app: Page) => (await callsTo(app)).filter(([command]) => command === "core_message_note_set").map(([, args]) => args);

test("a text in a circle is translated in its bubble, under who said it, and the original is one tap away", async ({ page: app }) => {
  await circle(app, { downloaded: true });
  const bubble = bobsBubble(app);
  await expect(bubble).toContainText(SAID);
  await expect(app.getByTestId("sender")).toHaveText("Bob");

  await bubble.getByTestId("translate").click();
  await expect(bubble).toContainText(TRANSLATED);
  await expect(bubble).not.toContainText(SAID);
  expect(await notesKept(app)).toEqual([{ message: "cm1", plugin: TRANSLATOR, text: TRANSLATED }]);

  await bubble.getByTestId("show-original").click();
  await expect(bubble).toContainText(SAID);
  await bubble.getByTestId("translate").click();
  await expect(bubble).toContainText(TRANSLATED);
  expect(await notesKept(app)).toHaveLength(1);
});

test("a missing language pack is asked about in an alert in the circle, and translated after a yes", async ({ page: app }) => {
  await circle(app);
  const bubble = bobsBubble(app);
  await bubble.getByTestId("translate").click();
  const alert = app.locator("ion-alert");
  await expect(alert).toContainText("Download French? About 30 MB, once.");
  await alert.locator("button", { hasText: "OK" }).click();
  await expect(bubble).toContainText(TRANSLATED);
});

test("with the circle's receiving switch on, texts arrive translated and the chip leads to its info", async ({ page: app }) => {
  await circle(app, { downloaded: true, flags: [{ plugin: TRANSLATOR, autoMessage: true, autoComposer: false, lang: "fr" }] });
  const bubble = bobsBubble(app);
  await expect(bubble).toContainText(TRANSLATED);
  await expect(bubble.getByTestId("show-original")).toBeVisible();
  const chip = app.getByTestId("translating");
  await expect(chip).toHaveText("Translating · French");
  await chip.click();
  await expect(app).toHaveURL(new RegExp(`/circle/${CIRCLE}/info$`));
});

test("with the circle's sending switch on, the text goes translated and my words stay under it", async ({ page: app }) => {
  await circle(app, { downloaded: true, flags: [{ plugin: TRANSLATOR, autoMessage: false, autoComposer: true, lang: "fr" }] });
  await app.locator("ion-textarea textarea").fill("hello");
  await app.getByTestId("circle-send").click();
  await expect.poll(async () => (await callsTo(app)).find(([command]) => command === "core_circle_send")?.[1]).toEqual({ circle: CIRCLE, text: "bonjour" });
  await expect.poll(async () => (await notesKept(app)).length).toBe(1);
  const [note] = await notesKept(app);
  expect(note).toMatchObject({ plugin: TRANSLATOR, text: "hello" });
  expect(String(note?.message)).toMatch(new RegExp(`^${CIRCLE}-`));

  const mine = app.locator(`[data-message='${String(note?.message)}'] [data-test='bubble']`);
  await expect(mine).toContainText("hello");
  const sentIn = mine.getByTestId("sent-in");
  await expect(sentIn).toHaveText("Sent in French · View");
  await sentIn.click();
  await expect(mine).toContainText("bonjour");
  await expect(mine).not.toContainText("hello");
});

test("the circle's info page sets it to translate what arrives, and the circle does so", async ({ page: app }) => {
  await circle(app, { downloaded: true });
  await expect(bobsBubble(app).getByTestId("translate")).toBeVisible();
  await expect(app.getByTestId("translating")).toHaveCount(0);

  await app.getByTestId("circle-peer").click();
  await expect(app).toHaveURL(new RegExp(`/circle/${CIRCLE}/info$`));
  const group = app.getByTestId(`settings-${TRANSLATOR}`);
  await expect(group).toContainText("Translation");
  await expect(group).toContainText("Language packs are downloaded once; translation happens on the phone.");
  const receive = group.getByTestId("setting-receive");
  await expect(receive).toBeEnabled();
  await receive.click();
  await expect
    .poll(async () => (await callsTo(app)).filter(([command]) => command === "core_chat_hooks_set").at(-1)?.[1])
    .toEqual({ conversation: CIRCLE, plugin: TRANSLATOR, autoMessage: true, autoComposer: false, lang: "fr" });

  await app.locator("ion-back-button:visible").click();
  await expect(app).toHaveURL(new RegExp(`/circle/${CIRCLE}$`));
  await expect(app.getByTestId("translating")).toHaveText("Translating · French");
  await expect(bobsBubble(app)).toContainText(TRANSLATED);
});
