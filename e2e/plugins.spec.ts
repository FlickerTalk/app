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

test("a plugin without the permission gets nothing into the chat", async ({ app }) => {
  await openTool(app, "com.flickertalk.sketch");
  await frameSays(app, { type: "ft.made", name: "drawing.png", mime: "image/png", data: "AAAA" });
  await frameSays(app, { type: "ft.text", text: "look at this" });
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

// 2026-10-03 (Ioan): the app's colours reach the plugin through the real frame bridge
// (`src-tauri/src/frame.js`, as the app serves it): Ionic's variables on the frame's root and
// `data-dark`, there when the plugin draws and following the app's look while it is open.
async function serveRealFrames(app: import("@playwright/test").Page) {
  const bridge = readFileSync(fileURLToPath(new URL("../src-tauri/src/frame.js", import.meta.url)), "utf8");
  // A plugin that keeps what it was opened with, and the colour it saw when it was created.
  const plugin = `customElements.define("ft-markdown", class extends HTMLElement {
    connectedCallback() { window.created = getComputedStyle(document.documentElement).getPropertyValue("--ion-text-color").trim(); }
  });
  window.opened = [];
  ft.onOpen((one) => window.opened.push(one));`;
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
