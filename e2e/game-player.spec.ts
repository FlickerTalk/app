import type { Page } from "@playwright/test";
import { commandsSent, expect, frameAsks, servePluginFrames, test } from "./helpers";

const BOB = "ft_bob123456789";
const HIDDEN = "ft_hidden1234567";
const TICTACTOE = "com.flickertalk.game.tictactoe";
const JUNO = { who: "juno", hair: "curly-puff", hairColour: "copper", name: "Ana" };
const MILO = { who: "milo", hair: "bob", hairColour: "ink", name: "Leo" };

async function play(app: Page, contact: string) {
  await app.goto(`/chat/${contact}`);
  await app.getByTestId("apps").click();
  await app.getByTestId("apps-tab-games").click();
  await app.getByTestId(`game-${TICTACTOE}`).click();
  const allow = app.getByTestId("game-allow");
  await expect(app.getByTestId("game-room").or(allow)).toBeVisible();
  if (await allow.isVisible()) await allow.click();
  await expect(app.getByTestId("game-room")).toBeVisible();
}

async function openSession(app: Page, pin: string) {
  await app.goto("/session");
  for (const digit of pin) await app.getByTestId(`key-${digit}`).click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
}

function keptPlaces(app: Page): Promise<string[]> {
  return app.evaluate(() => Object.keys((window as unknown as { __ftFake: { state: { pluginData: object } } }).__ftFake.state.pluginData));
}

test("a game keeps the player in the app and finds it again after the app starts again", async ({ app }) => {
  await servePluginFrames(app);
  await play(app, BOB);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q1" })).toBeNull();
  expect(await frameAsks(app, { type: "ft.playerSet", id: "q2", player: JUNO })).toBe(true);
  expect(await commandsSent(app)).toContain("core_set_game_player");

  await app.reload();
  await play(app, BOB);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q3" })).toEqual(JUNO);
});

test("a tool gets no player and keeps none", async ({ app }) => {
  await servePluginFrames(app);
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  await app.getByTestId("app-com.flickertalk.markdown").click();
  expect(await frameAsks(app, { type: "ft.playerSet", id: "q1", player: JUNO })).toBe(false);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q2" })).toBeNull();
  expect(await keptPlaces(app)).toEqual([]);
});

test("a player chosen inside a session stays inside it and goes with it", async ({ app }) => {
  await servePluginFrames(app);
  await play(app, BOB);
  expect(await frameAsks(app, { type: "ft.playerSet", id: "q1", player: JUNO })).toBe(true);

  await openSession(app, "777777");
  await play(app, HIDDEN);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q2" })).toBeNull();
  expect(await frameAsks(app, { type: "ft.playerSet", id: "q3", player: MILO })).toBe(true);
  await play(app, BOB);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q4" })).toEqual(JUNO);
  await play(app, HIDDEN);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q5" })).toEqual(MILO);
  expect(await keptPlaces(app)).toHaveLength(2);

  await app.goto("/tabs/chats");
  const section = app.getByTestId("session-section");
  await section.getByTestId("session-remove").click();
  await section.getByTestId("session-remove-sure").click();
  await expect(section).toHaveCount(0);
  expect(await keptPlaces(app)).toHaveLength(1);
  await play(app, BOB);
  expect(await frameAsks(app, { type: "ft.playerGet", id: "q6" })).toEqual(JUNO);
});
