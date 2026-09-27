// A2 of the 2026-09-24 review: what a plugin makes goes only as far as the user allowed. With
// `propose` it waits in the composer for the user to send; with nothing granted, nothing leaves.
import { callsTo, expect, frameSays, test } from "./helpers";

async function openTool(app: import("@playwright/test").Page, id: string) {
  await app.goto("/chat/ft_bob123456789");
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
