// 2026-10-20 (plan of the translator §8.2d, approved UX): translating inside a 1:1 conversation.
// The header and the composer stay as they were. A received text offers "Translate" inside its
// bubble; the tool runs in a frame out of sight (the real `frame.js`, with a translator of the test's
// own) and its answer takes the text's place, with "Show original" to switch back. A language pack
// missing, the tool asks in its answer and the app asks the user in an alert. With the conversation's
// switches on, what arrives is translated by itself, a chip under the header says so, and what is
// sent goes in the conversation's language with the user's words kept under it.
import { devices, type Locator, type Page } from "@playwright/test";
import { test as base } from "@playwright/test";
import { installFakeCore } from "./fake-core";
import { callsTo, expect, serveRealPluginFrames } from "./helpers";

// The conversation's switches are read as the fake core is installed: they go in first.
const test = base;

const TRANSLATOR = "com.flickertalk.translate";
const BOB = "ft_bob123456789";
const FRENCH = "Coucou, on se voit samedi ?";
const SPANISH = "Hola, ¿nos vemos el sábado?";

/**
 * A translator as the real one answers the app: a question first while its pack is missing (asked
 * by the user's own runs; the pack is the phone's, so a run of the conversation's own just goes),
 * and the conversation's settings, French found in what Bob writes.
 */
const plugin = (downloaded: boolean) => `
customElements.define("ft-probe", class extends HTMLElement {});
let downloaded = ${downloaded};
let record = { lang: "fr", receive: false, send: false };
const SPANISH = { ${JSON.stringify(FRENCH)}: ${JSON.stringify(SPANISH)} };
const FRENCH = { "hello": "bonjour" };
const ASK = { ask: { text: "Download French? About 30 MB, once." } };
ft.onHook(async (hook) => {
  if (hook.name === "message") {
    if (!downloaded && hook.manual && !hook.answer) return ASK;
    if (hook.answer) downloaded = true;
    return { text: SPANISH[hook.text] ?? "" };
  }
  if (hook.name === "composer") return { text: FRENCH[hook.text] ?? hook.text };
  if (hook.name === "settings") {
    if (hook.values) {
      const next = { ...record, ...hook.values };
      if ((next.receive || next.send) && !downloaded && !hook.answer) return ASK;
      if (hook.answer) downloaded = true;
      record = next;
    }
    return {
      values: record,
      options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French (detected)" }] },
      auto: { message: Boolean(record.lang && record.receive), composer: Boolean(record.lang && record.send) },
      lang: record.lang || null,
    };
  }
  return null;
});
`;

async function conversation(app: Page, options: { downloaded?: boolean; flags?: Record<string, unknown>[] } = {}) {
  await app.addInitScript(
    ({ french, flags, bob }) => {
      const fake = window as unknown as Record<string, unknown>;
      fake.__ftFakeTranslateTool = true;
      fake.__ftFakeTranslateReads = true;
      fake.__ftFakeBobSays = [french];
      if (flags) fake.__ftFakeChatHooks = { [bob]: flags };
    },
    { french: FRENCH, flags: options.flags, bob: BOB },
  );
  await app.addInitScript(installFakeCore);
  await serveRealPluginFrames(app, plugin(options.downloaded ?? false));
  await app.goto(`/chat/${BOB}`);
}

/** The bubble of the text Bob wrote in French. */
const bobsBubble = (app: Page) => app.locator("[data-message='said0'] [data-test='bubble']");

test("a received text is translated in its bubble, and the original is one tap away", async ({ page: app }) => {
  await conversation(app, { downloaded: true });
  const bubble = bobsBubble(app);
  await expect(bubble).toContainText(FRENCH);
  // The header and the composer as they were.
  await expect(app.locator("[data-test^='chat-hook-'], [data-test^='hook-composer-']")).toHaveCount(0);

  await bubble.getByTestId("translate").click();
  await expect(bubble).toContainText(SPANISH);
  await expect(bubble).not.toContainText(FRENCH);
  const notes = (await callsTo(app)).filter(([command]) => command === "core_message_note_set");
  expect(notes.map(([, args]) => args)).toEqual([{ message: "said0", plugin: TRANSLATOR, text: SPANISH }]);

  await bubble.getByTestId("show-original").click();
  await expect(bubble).toContainText(FRENCH);
  await bubble.getByTestId("translate").click();
  await expect(bubble).toContainText(SPANISH);
  // Shown again from the note: the tool is not asked twice.
  expect((await callsTo(app)).filter(([command]) => command === "core_message_note_set")).toHaveLength(1);
});

test("a missing language pack is asked about in an alert, and the text is translated after a yes", async ({ page: app }) => {
  await conversation(app);
  const bubble = bobsBubble(app);
  await bubble.getByTestId("translate").click();
  const alert = app.locator("ion-alert");
  await expect(alert).toBeVisible();
  await expect(alert).toContainText("Download French? About 30 MB, once.");
  await alert.locator("button", { hasText: "OK" }).click();
  await expect(bubble).toContainText(SPANISH);
});

test("nothing is kept when the user says no to the download", async ({ page: app }) => {
  await conversation(app);
  const bubble = bobsBubble(app);
  await bubble.getByTestId("translate").click();
  await app.locator("ion-alert button", { hasText: "Cancel" }).click();
  await expect(app.locator("ion-alert")).toHaveCount(0);
  await expect(bubble).toContainText(FRENCH);
  await expect(bubble.getByTestId("translate")).toBeVisible();
  expect((await callsTo(app)).filter(([command]) => command === "core_message_note_set")).toHaveLength(0);
});

test("with the receiving switch on, texts arrive translated and a chip says so", async ({ page: app }) => {
  await conversation(app, { downloaded: true, flags: [{ plugin: TRANSLATOR, autoMessage: true, autoComposer: false, lang: "fr" }] });
  const bubble = bobsBubble(app);
  await expect(bubble).toContainText(SPANISH);
  await expect(bubble.getByTestId("show-original")).toBeVisible();
  const chip = app.getByTestId("translating");
  await expect(chip).toHaveText("Translating · French");
  await chip.click();
  await expect(app).toHaveURL(new RegExp(`/contact/${BOB}$`));
});

test("the contact's page turns translating what arrives on, after the download, and the chat follows", async ({ page: app }) => {
  await conversation(app);
  await app.getByTestId("peer").click();
  await expect(app).toHaveURL(new RegExp(`/contact/${BOB}$`));
  const group = app.getByTestId(`settings-${TRANSLATOR}`);
  await expect(group).toContainText("Translation");
  await expect(group).toContainText("Language packs are downloaded once; translation happens on the phone.");
  await expect(group.getByTestId("setting-lang")).toHaveJSProperty("value", "fr");
  const receive = group.getByTestId("setting-receive");
  await expect(receive).toHaveJSProperty("disabled", false);

  await receive.click();
  // The page has an alert of its own (deleting the contact): the tool's question is the one asking.
  const alert = app.locator("ion-alert", { hasText: "Download French? About 30 MB, once." });
  await expect(alert).toBeVisible();
  await alert.locator("button", { hasText: "OK" }).click();
  await expect(receive).toHaveJSProperty("checked", true);
  await expect
    .poll(async () => (await callsTo(app)).filter(([command]) => command === "core_chat_hooks_set").map(([, args]) => args).at(-1))
    .toEqual({ conversation: BOB, plugin: TRANSLATOR, autoMessage: true, autoComposer: false, lang: "fr" });

  await app.goBack();
  await expect(app.getByTestId("translating")).toHaveText("Translating · French");
  await expect(bobsBubble(app)).toContainText(SPANISH);
});

test("a no to the download leaves the switch off", async ({ page: app }) => {
  await conversation(app);
  await app.getByTestId("peer").click();
  const receive = app.getByTestId(`settings-${TRANSLATOR}`).getByTestId("setting-receive");
  await expect(receive).toHaveJSProperty("disabled", false);
  await receive.click();
  await app.locator("ion-alert", { hasText: "Download French?" }).locator("button", { hasText: "Cancel" }).click();
  await expect(receive).toHaveJSProperty("checked", false);
  const kept = (await callsTo(app)).filter(([command]) => command === "core_chat_hooks_set").map(([, args]) => args);
  expect(kept.some((args) => (args as { autoMessage?: boolean }).autoMessage)).toBe(false);
});

test("with the sending switch on, what I write goes in French and my bubble keeps my words", async ({ page: app }) => {
  await conversation(app, { downloaded: true, flags: [{ plugin: TRANSLATOR, autoMessage: false, autoComposer: true, lang: "fr" }] });
  await expect(app.getByTestId("translating")).toHaveText("Translating · French");
  await app.locator("ion-textarea textarea").fill("hello");
  await app.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(async () => (await callsTo(app)).filter(([command]) => command === "core_send").map(([, args]) => args?.text)).toEqual(["bonjour"]);

  const mine = app.locator("[data-message='sent1'] [data-test='bubble']");
  await expect(mine).toContainText("hello");
  const sentIn = mine.getByTestId("sent-in");
  await expect(sentIn).toHaveText("Sent in French · View");
  await sentIn.click();
  await expect(mine).toContainText("bonjour");
  await mine.getByTestId("show-original").click();
  await expect(mine).toContainText("hello");
});

// QA of the translator (2026-10-20, D6): on an iPhone, Ionic draws a list header as a large title
// (22 px, its content at the bottom), so the Translation group's header stood out from the contact
// page's other groups and its icon sat below the middle of its title. It is drawn as on Android: no
// larger than its rows, its icon centred on its title and in the column of the other groups' icons.
const PHONES = [
  { name: "an Android phone", mode: "md", use: {} },
  { name: "an iPhone", mode: "ios", use: { userAgent: devices["iPhone 13"].userAgent, viewport: { width: 390, height: 844 } } },
];
for (const phone of PHONES) {
  test.describe(`on ${phone.name}`, () => {
    test.use(phone.use);

    test("the contact's Translation group has a header no larger than its rows, its icon centred on its title", async ({ page: app }) => {
      await conversation(app);
      await app.getByTestId("peer").click();
      await expect(app.locator("html")).toHaveClass(new RegExp(`\\b${phone.mode}\\b`));
      const group = app.getByTestId(`settings-${TRANSLATOR}`);
      const header = group.locator("ion-list-header");
      await expect(header).toContainText("Translation");
      await group.scrollIntoViewIfNeeded();

      const fontSize = (one: Locator) => one.evaluate((element) => parseFloat(getComputedStyle(element).fontSize));
      expect(await fontSize(header.locator("ion-label"))).toBeLessThanOrEqual(await fontSize(group.getByTestId("setting-receive")));

      const icon = (await header.locator("ion-icon").boundingBox())!;
      const title = (await header.locator("ion-label").boundingBox())!;
      expect(Math.abs(icon.y + icon.height / 2 - (title.y + title.height / 2)), "the icon's middle against the title's").toBeLessThanOrEqual(1);
      // The other groups' icons: the rules group's, the one just above.
      const ruleIcon = (await app.locator("ion-item", { has: app.getByTestId("typing") }).locator("ion-icon").boundingBox())!;
      expect(Math.abs(icon.x - ruleIcon.x), "the icon's column against the rules group's").toBeLessThanOrEqual(1);
    });
  });
}
