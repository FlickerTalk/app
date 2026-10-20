// 2026-10-20 (plan of the translator §8.2d, approved UX): translating inside a 1:1 conversation.
// The header and the composer stay as they were. A received text offers "Translate" inside its
// bubble; the tool runs in a frame out of sight (the real `frame.js`, with a translator of the test's
// own) and its answer takes the text's place, with "Show original" to switch back. A language pack
// missing, the tool asks in its answer and the app asks the user in an alert. With the conversation's
// switches on, what arrives is translated by itself, a chip under the header says so, and what is
// sent goes in the conversation's language with the user's words kept under it.
import type { Page } from "@playwright/test";
import { test as base } from "@playwright/test";
import { installFakeCore } from "./fake-core";
import { callsTo, expect, serveRealPluginFrames } from "./helpers";

// The conversation's switches are read as the fake core is installed: they go in first.
const test = base;

const TRANSLATOR = "com.flickertalk.translate";
const BOB = "ft_bob123456789";
const FRENCH = "Coucou, on se voit samedi ?";
const SPANISH = "Hola, ¿nos vemos el sábado?";

/** A translator as the real one answers the app: a question first while its pack is missing. */
const plugin = (downloaded: boolean) => `
customElements.define("ft-probe", class extends HTMLElement {});
let downloaded = ${downloaded};
const SPANISH = { ${JSON.stringify(FRENCH)}: ${JSON.stringify(SPANISH)}, "hello": "bonjour" };
ft.onHook(async (hook) => {
  if (hook.name === "message") {
    if (!downloaded && !hook.answer) return hook.manual ? { ask: { text: "Download French? About 30 MB, once." } } : null;
    downloaded = true;
    return { text: SPANISH[hook.text] ?? "" };
  }
  if (hook.name === "composer") return { text: SPANISH[hook.text] ?? hook.text };
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
