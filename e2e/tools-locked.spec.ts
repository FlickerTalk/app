// Ioan, 2026-10-08: after the 15 free days, without the subscription, only the premium part is
// locked: the tools and the extra sessions with a PIN. Each shows a lock and leads to the Plan
// screen; games, chat, calls and files never ask.
import { callsTo, expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

test.beforeEach(async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__ftFakeLimited = true;
  });
});

test("a tool in the chat's apps sheet shows a lock and leads to the Plan screen", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  const tool = app.getByTestId("app-com.flickertalk.markdown");
  await expect(tool.getByTestId("locked")).toBeVisible();
  await tool.click();
  await expect(app).toHaveURL(/\/plan$/);
  await expect(app.getByTestId("hint")).toContainText("Chat, calls, files and games are free, forever");
  await expect(app.getByTestId("pay")).toBeVisible();
  await expect(app.getByTestId("restore")).toBeVisible();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
});

test("a game is never locked", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  await app.getByTestId("apps-tab-games").click();
  const game = app.getByTestId(`game-${TICTACTOE}`);
  await expect(game).toBeVisible();
  await expect(game.getByTestId("locked")).toHaveCount(0);
  await game.click();
  await expect(app).not.toHaveURL(/\/plan$/);
});

test("the tools page locks every tool and installs nothing", async ({ app }) => {
  await app.goto("/plugins");
  await expect(app.getByTestId("locked")).toContainText("tools need the subscription");
  await app.getByTestId("open-com.flickertalk.markdown").click();
  await expect(app).toHaveURL(/\/plan$/);
  expect((await callsTo(app)).some(([command]) => command === "core_plugin_add")).toBe(false);
});

test("a conversation still goes on: writing is never locked", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.locator("ion-textarea textarea").fill("still free");
  await app.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(async () => (await callsTo(app)).some(([command]) => command === "core_send")).toBe(true);
});

// §108: the lock stands in front of the PIN pad, never after a PIN.
test("an extra session with a PIN is locked in front of the pad", async ({ app }) => {
  await app.goto("/tabs/settings");
  await expect(app.getByTestId("session-locked")).toBeVisible();
  await app.getByTestId("session").click();
  await expect(app).toHaveURL(/\/plan$/);
  await app.goto("/session");
  await expect(app.getByTestId("locked")).toBeVisible();
  await expect(app.getByTestId("key-1")).toHaveCount(0);
  expect((await callsTo(app)).some(([command]) => command === "core_session_open")).toBe(false);
});
