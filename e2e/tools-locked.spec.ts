// Ioan, 2026-10-08: after the 15 free days, without the subscription, only the premium part is
// locked: the tools and the extra sessions with a PIN. Each shows a lock and leads to the Premium
// section of Settings (the Plan screen is gone, mockup of 2026-10-08); games, chat, calls and
// files never ask.
import type { Page } from "@playwright/test";
import { callsTo, expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

test.beforeEach(async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__ftFakeLimited = true;
  });
});

/** The Premium section of Settings, in view, with the subscription and the restore. */
async function atPremium(app: Page) {
  await expect(app).toHaveURL(/\/tabs\/settings#premium$/);
  const premium = app.locator(".ion-page:not(.ion-page-hidden) [data-test='premium']");
  await expect(premium.getByTestId("premium-note")).toContainText("Chat, calls, files and games stay free");
  await expect(premium.getByTestId("pay")).toBeInViewport();
  await expect(premium.getByTestId("restore")).toBeVisible();
}

test("a tool in the chat's apps sheet shows a lock and leads to the Premium section", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.getByTestId("apps").click();
  const tool = app.getByTestId("app-com.flickertalk.markdown");
  await expect(tool.getByTestId("locked")).toBeVisible();
  await tool.click();
  await atPremium(app);
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
  await expect(app).not.toHaveURL(/premium/);
});

// 2026-10-08 (plan of the apps grid): the Apps tab locks each tool's tile, and its sheet offers
// the subscription; a game's tile has no lock.
test("the Apps tab locks every tool, offers the subscription on its sheet, and installs nothing", async ({ app }) => {
  await app.goto("/tabs/apps");
  const tool = app.getByTestId("app-com.flickertalk.markdown");
  await expect(tool.getByTestId("locked")).toBeVisible();
  await expect(tool).toHaveAttribute("aria-label", "Markdown, locked");
  await tool.click({ button: "right" });
  await expect(app.getByTestId("sheet-open")).toContainText("Subscribe");
  await app.getByTestId("sheet-open").click();
  await atPremium(app);

  await app.goto("/tabs/apps?show=games");
  await expect(app.getByTestId(`app-${TICTACTOE}`).getByTestId("locked")).toHaveCount(0);
  expect((await callsTo(app)).some(([command]) => command === "core_plugin_add")).toBe(false);
});

test("a conversation still goes on: writing is never locked", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.locator("ion-textarea textarea").fill("still free");
  await app.getByRole("button", { name: "Send", exact: true }).click();
  await expect.poll(async () => (await callsTo(app)).some(([command]) => command === "core_send")).toBe(true);
});

// §108: the lock stands in front of the PIN pad, never after a PIN. In Settings both premium rows
// are locked and a tap on one asks the Store, as the Subscribe button above them does.
test("the premium rows of Settings are locked and lead to the subscription", async ({ app }) => {
  await app.goto("/tabs/settings");
  for (const row of ["plugins", "session"]) {
    await expect(app.getByTestId(row).getByTestId("premium-locked")).toBeVisible();
    await app.getByTestId(row).click();
    await expect(app).toHaveURL(/\/tabs\/settings$/);
  }
  await expect.poll(async () => (await callsTo(app)).filter(([command]) => command === "core_subscribe").length).toBe(2);
  expect((await callsTo(app)).some(([command]) => command === "core_session_open")).toBe(false);
});

test("the PIN pad is locked in front of the keys", async ({ app }) => {
  await app.goto("/session");
  await expect(app.getByTestId("locked")).toBeVisible();
  await expect(app.getByTestId("key-1")).toHaveCount(0);
  await app.getByTestId("subscribe").click();
  await atPremium(app);
  expect((await callsTo(app)).some(([command]) => command === "core_session_open")).toBe(false);
});

// The old address of the Plan screen still works: it lands on the section.
test("the old Plan address opens the Premium section", async ({ app }) => {
  await app.goto("/plan");
  await atPremium(app);
});
