// A2 of the 2026-09-24 review: what a plugin makes goes only as far as the user allowed. With
// `propose` it waits in the composer for the user to send; with nothing granted, nothing leaves.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { callsTo, expect, frameAsks, frameHeard, frameSays, servePluginFrames, test } from "./helpers";

async function openTool(app: import("@playwright/test").Page, id: string, chat = "ft_bob123456789") {
  await app.goto(`/chat/${chat}`);
  await app.getByTestId("apps").click();
  await app.getByTestId(`app-${id}`).click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
}

test("a plugin with the propose permission stages its file; the user sends it", async ({ app }) => {
  await openTool(app, "com.flickertalk.markdown");
  await frameSays(app, { type: "ft.made", name: "notes.md", mime: "text/markdown", data: "IyBoaQ==" });

  const staged = app.getByTestId("staged");
  await expect(staged).toBeVisible();
  await expect(staged).toContainText("notes.md");
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  let calls = await callsTo(app);
  expect(calls.some(([command, args]) => command === "core_plugin_made" && args?.plugin === "com.flickertalk.markdown")).toBe(true);
  expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);

  await app.getByTestId("staged-send").click();
  await expect(app.getByTestId("staged")).toHaveCount(0);
  calls = await callsTo(app);
  const sent = calls.find(([command]) => command === "core_send_picked");
  expect(sent?.[1]).toMatchObject({ contact: "ft_bob123456789", file: { name: "notes.md" } });
});

// Since 2026-10-08 (Ioan) a plugin that asked for the permission asks the user on the spot; the
// user says no each time, and nothing reaches the chat.
test("a plugin without the permission gets nothing into the chat", async ({ app }) => {
  await openTool(app, "com.flickertalk.sketch");
  await frameSays(app, { type: "ft.made", name: "drawing.png", mime: "image/png", data: "AAAA" });
  await expect(app.getByTestId("permission-ask")).toBeVisible();
  await app.getByTestId("permission-cancel").click();
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  await frameSays(app, { type: "ft.text", text: "look at this" });
  await expect(app.getByTestId("permission-ask")).toBeVisible();
  await app.getByTestId("permission-cancel").click();
  await expect(app.getByTestId("permission-ask")).toHaveCount(0);
  // Nothing to stage, nothing in the composer, and the core was never asked.
  await expect(app.getByTestId("staged")).toHaveCount(0);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  const calls = await callsTo(app);
  expect(calls.some(([command]) => command === "core_plugin_made")).toBe(false);
  expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);
  await expect(app.locator("ion-textarea")).not.toContainText("look at this");
});

test("a proposed text lands in the composer, not on the wire", async ({ app }) => {
  await openTool(app, "com.flickertalk.markdown");
  await frameSays(app, { type: "ft.text", text: "# Title" });
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  await expect(app.locator("ion-textarea textarea")).toHaveValue("# Title");
  expect((await callsTo(app)).some(([command]) => command === "core_send")).toBe(false);
});

// 2026-10-01 (§108): what a plugin keeps from a conversation of a hidden session (a board, say)
// stays in that session. From the main list it is not there: not its value, its key, nor its size.
async function openHiddenSession(app: import("@playwright/test").Page) {
  await app.goto("/session");
  for (const digit of "777777") await app.getByTestId(`key-${digit}`).click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
}

test("a record kept from a hidden session's conversation is not there from the main list", async ({ app }) => {
  await servePluginFrames(app);
  await openHiddenSession(app);
  await openTool(app, "com.flickertalk.markdown", "ft_hidden1234567");
  expect(await frameAsks(app, { type: "ft.recordSet", id: "q1", key: "board/1", value: "the secret board" })).toBe(true);
  expect(await frameAsks(app, { type: "ft.recordKeys", id: "q2", prefix: "" })).toEqual(["board/1"]);
  const kept = (await callsTo(app)).find(([command]) => command === "core_plugin_record_set");
  expect(kept?.[1]).toMatchObject({ plugin: "com.flickertalk.markdown", key: "board/1", session: "s1" });
  // The plugin never hears of the session.
  expect(JSON.stringify(await frameHeard(app))).not.toContain("s1");

  await openTool(app, "com.flickertalk.markdown");
  expect(await frameAsks(app, { type: "ft.recordKeys", id: "q3", prefix: "" })).toEqual([]);
  expect(await frameAsks(app, { type: "ft.recordGet", id: "q4", key: "board/1" })).toBeNull();
  expect(await frameAsks(app, { type: "ft.recordUsage", id: "q5" })).toEqual({ used: 0, quota: 4 * 1024 * 1024 });
  const asked = (await callsTo(app)).filter(([command]) => command === "core_plugin_record_keys");
  expect(asked.at(-1)?.[1]?.session).toBeUndefined();
});

test("a record kept from the main list is there as before", async ({ app }) => {
  await servePluginFrames(app);
  await openTool(app, "com.flickertalk.markdown");
  expect(await frameAsks(app, { type: "ft.recordSet", id: "q1", key: "board/1", value: "a board" })).toBe(true);
  await openTool(app, "com.flickertalk.markdown");
  expect(await frameAsks(app, { type: "ft.recordGet", id: "q2", key: "board/1" })).toBe("a board");
  expect(await frameAsks(app, { type: "ft.recordKeys", id: "q3", prefix: "board/" })).toEqual(["board/1"]);
});

// 2026-10-02 (finding 7 of the plan of the plugins): opened in a chat, a plugin learns the core's
// opaque id of that chat, the same each time; another chat has another; opened on its own, none.
test("a plugin opened in a chat learns that chat's id, and none on its own", async ({ app }) => {
  await servePluginFrames(app);
  const chatOf = async () => {
    await expect.poll(async () => (await frameHeard(app)).length).toBeGreaterThan(0);
    const opened = (await frameHeard(app)).find((one) => (one as { type?: string }).type === "ft.open") as Record<string, unknown>;
    return opened;
  };
  await openHiddenSession(app);
  await openTool(app, "com.flickertalk.markdown", "ft_hidden1234567");
  const hidden = (await chatOf()).chat;

  await openTool(app, "com.flickertalk.markdown");
  const bob = (await chatOf()).chat;
  expect(bob).toMatch(/^[A-Za-z0-9_-]{43}$/);
  expect(String(bob)).not.toContain("bob123456789");
  expect(hidden).toMatch(/^[A-Za-z0-9_-]{43}$/);
  expect(hidden).not.toBe(bob);
  await openTool(app, "com.flickertalk.markdown");
  expect((await chatOf()).chat).toBe(bob);

  await app.goto("/plugin/com.flickertalk.markdown");
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  expect(await chatOf()).not.toHaveProperty("chat");
});

// 2026-10-02 (Ioan): the app's colours reach the plugin through the real frame bridge
// (`src-tauri/src/frame.js`, as the app serves it): Ionic's variables on the frame's root and
// `data-dark`, there when the plugin draws and following the app's look while it is open.
// A plugin that keeps what it was opened with, and the colour it saw when it was created.
const KEEPS_WHAT_IT_SAW = `customElements.define("ft-markdown", class extends HTMLElement {
    connectedCallback() { window.created = getComputedStyle(document.documentElement).getPropertyValue("--ion-text-color").trim(); }
  });
  window.opened = [];
  ft.onOpen((one) => window.opened.push(one));`;

async function serveRealFrames(app: import("@playwright/test").Page, plugin = KEEPS_WHAT_IT_SAW) {
  const bridge = readFileSync(fileURLToPath(new URL("../src-tauri/src/frame.js", import.meta.url)), "utf8");
  await app.route("http://ftplugin.localhost/**", (route) => {
    const path = new URL(route.request().url()).pathname;
    if (path.endsWith("/frame.js")) return route.fulfill({ contentType: "text/javascript", body: bridge });
    if (path.endsWith("/dist/index.js")) return route.fulfill({ contentType: "text/javascript", body: plugin });
    return route.fulfill({
      contentType: "text/html",
      body: '<!doctype html><html><head><script type="module" src="./frame.js"></script></head><body><ft-markdown id="view"></ft-markdown></body></html>',
    });
  });
}

test("a plugin wears the app's colours, and follows the app from dark to light while it is open", async ({ app }) => {
  await app.addInitScript(() => localStorage.setItem("ft-appearance", "system"));
  await app.emulateMedia({ colorScheme: "dark" });
  await serveRealFrames(app);
  await openTool(app, "com.flickertalk.markdown");
  const appText = () => app.evaluate(() => getComputedStyle(document.body).getPropertyValue("--ion-text-color").trim());
  const inside = async () => {
    const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"));
    if (!frame) return null;
    return frame.evaluate(() => {
      const root = document.documentElement;
      const seen = window as unknown as { created?: string; opened?: { dark: boolean; theme: Record<string, string> }[] };
      return {
        text: getComputedStyle(root).getPropertyValue("--ion-text-color").trim(),
        dark: root.dataset.dark ?? null,
        created: seen.created ?? null,
        openedDark: seen.opened?.[0]?.dark ?? null,
      };
    });
  };

  const dark = await appText();
  expect(dark).toBe("#f5f5f5");
  await expect.poll(inside).toEqual({ text: dark, dark: "1", created: dark, openedDark: true });

  await app.emulateMedia({ colorScheme: "light" });
  // The app follows the system's change first (theme.ts), then the plugin follows the app.
  await expect.poll(appText).toBe("#0a0a0a");
  const light = await appText();
  await expect.poll(inside).toMatchObject({ text: light, dark: null });
});

// 2026-10-04: the frame follows the plugin's content down as well as up. A game's waiting screen is
// tall and its board is short: once the board shows, the window shrinks to it and leaves no empty
// area to scroll under it.
test("a plugin's window shrinks when its content does", async ({ app }) => {
  await serveRealFrames(
    app,
    `customElements.define("ft-markdown", class extends HTMLElement {
      connectedCallback() {
        this.style.display = "block";
        this.style.height = "1200px";
        window.shrink = () => { this.style.height = "200px"; };
      }
    });`,
  );
  await openTool(app, "com.flickertalk.markdown");
  const tall = () => app.locator("iframe.ft-plugin__frame").evaluate((frame) => frame.getBoundingClientRect().height);
  await expect.poll(tall).toBeGreaterThan(1200);

  const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"))!;
  await frame.evaluate(() => (window as unknown as { shrink: () => void }).shrink());
  await expect.poll(tall).toBeLessThan(260);
});

// 2026-10-02 (seen on two phones and the iOS simulator): closed by the app, a plugin in a live
// session vanished without a word and the other side kept saying both were there. Now the app tells
// it first (the real `frame.js` runs its `ft.onClose`), so its goodbye reaches the core.
const MARKDOWN = "com.flickertalk.markdown";
const TICTACTOE = "com.flickertalk.game.tictactoe";
const SAYS_BYE = `customElements.define("ft-markdown", class extends HTMLElement {});
  ft.onOpen(() => (window.opened = true));
  ft.onClose(() => ft.live.send("Ynll"));`;

/** What reached the core over the live channel, oldest first. */
async function liveSent(app: import("@playwright/test").Page) {
  return (await callsTo(app)).filter(([command]) => command === "core_plugin_live_send").map(([, args]) => args);
}

/** The plugin's frame is up and was opened: from then on it has something to say goodbye to. */
async function pluginOpened(app: import("@playwright/test").Page) {
  await expect
    .poll(async () => {
      const frame = app.frames().find((one) => one.url().startsWith("http://ftplugin.localhost/"));
      return frame ? frame.evaluate(() => Boolean((window as unknown as { opened?: boolean }).opened)).catch(() => false) : false;
    })
    .toBe(true);
}

async function openLiveTool(app: import("@playwright/test").Page, plugin = SAYS_BYE) {
  await app.addInitScript((id) => ((window as unknown as Record<string, unknown>).__ftFakeLivePlugins = [id]), MARKDOWN);
  await serveRealFrames(app, plugin);
  await openTool(app, MARKDOWN);
  await pluginOpened(app);
}

test("a plugin says goodbye to its twin when the app's ✕ closes it", async ({ app }) => {
  await openLiveTool(app);
  expect(await liveSent(app)).toEqual([]);
  await app.getByTestId("close-app").click();
  await expect(app.locator(".ft-app")).toBeHidden();
  await expect.poll(() => liveSent(app)).toEqual([{ plugin: MARKDOWN, contact: "ft_bob123456789", data: "Ynll" }]);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
});

test("a plugin says goodbye to its twin when Android's Back closes it, and the chat stays", async ({ app }) => {
  await openLiveTool(app);
  expect(await app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back())).toBe(true);
  await expect.poll(() => liveSent(app)).toEqual([{ plugin: MARKDOWN, contact: "ft_bob123456789", data: "Ynll" }]);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
});

// One press closes the plugin, and only that: pressed again while the plugin says goodbye (here,
// for the whole wait), Back is still the app's, so the chat stays and the goodbye goes once.
test("Back pressed again while a plugin says goodbye does not leave the chat", async ({ app }) => {
  await openLiveTool(
    app,
    `customElements.define("ft-markdown", class extends HTMLElement {});
    ft.onOpen(() => (window.opened = true));
    ft.onClose(async () => {
      await ft.live.send("Ynll");
      await new Promise(() => {});
    });`,
  );
  const press = () => app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());
  expect(await press()).toBe(true);
  await expect.poll(() => liveSent(app)).toHaveLength(1);
  // Still inside the wait (CLOSING_WAIT, 400 ms): the press is taken, and does nothing.
  expect(await press()).toBe(true);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  expect(await liveSent(app)).toEqual([{ plugin: MARKDOWN, contact: "ft_bob123456789", data: "Ynll" }]);
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  // Gone, Back is the system's again.
  expect(await press()).toBe(false);
});

test("a game says goodbye to its twin when the game room's ✕ closes it", async ({ app }) => {
  await serveRealFrames(app, SAYS_BYE);
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId(`game-${TICTACTOE}`).click();
  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId("game-room")).toBeVisible();
  await pluginOpened(app);

  await app.getByTestId("close-game").click();
  await expect(app.getByTestId("game-room")).toBeHidden();
  await expect(app.getByTestId("game-strip")).toHaveCount(0);
  await expect.poll(() => liveSent(app)).toEqual([{ plugin: TICTACTOE, contact: "ft_bob123456789", data: "Ynll" }]);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
});

// Leaving the conversation by its back arrow takes the page down at the end of Ionic's transition,
// with the game in it: the game is closed as the page starts to go, so its goodbye gets out. A page
// only covered by another one keeps the game, and says nothing.
test("a game says goodbye to its twin when the chat is left by its back arrow, and not when it is only covered", async ({ app }) => {
  await serveRealFrames(app, SAYS_BYE);
  await app.goto("/tabs/chats");
  await app.locator(".ft-row", { hasText: "see you at six" }).first().click();
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  await app.getByTestId("apps").click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId(`game-${TICTACTOE}`).click();
  await app.getByTestId("game-allow").click();
  await expect(app.getByTestId("game-room")).toBeVisible();
  await pluginOpened(app);

  // Covered by the contact's page, and back: the game is still there, and said nothing.
  await app.getByTestId("peer").click();
  await expect(app).toHaveURL(/\/contact\/ft_bob123456789$/);
  await app.goBack();
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  await expect(app.getByTestId("game-room")).toBeVisible();
  expect(await liveSent(app)).toEqual([]);

  await app.locator(".ft-thread__bar ion-back-button").last().click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect.poll(() => liveSent(app)).toEqual([{ plugin: TICTACTOE, contact: "ft_bob123456789", data: "Ynll" }]);
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
});

// A goodbye that never ends does not keep the plugin: the app lets it go after a few tenths.
test("a plugin whose goodbye never ends is let go all the same", async ({ app }) => {
  await openLiveTool(
    app,
    `customElements.define("ft-markdown", class extends HTMLElement {});
    ft.onOpen(() => (window.opened = true));
    ft.onClose(() => new Promise(() => {}));`,
  );
  await app.getByTestId("close-app").click();
  await expect(app.locator(".ft-app")).toBeHidden();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0, { timeout: 2000 });
});
