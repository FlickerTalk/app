// A2 of the 2026-09-24 review: what a plugin makes goes only as far as the user allowed. With
// `propose` it waits in the composer for the user to send; with nothing granted, nothing leaves.
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
