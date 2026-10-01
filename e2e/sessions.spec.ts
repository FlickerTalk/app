// A3: every PIN opens a session the same way, the one that has it or a new empty one; an empty
// one goes when it is closed, and any session can be deleted.
import { commandsSent, expect, test } from "./helpers";

async function type(app: import("@playwright/test").Page, digits: string) {
  for (const digit of digits) {
    await app.getByTestId(`key-${digit}`).click();
  }
}

test("every pin opens a session the same way, with no second gesture", async ({ app }) => {
  await app.goto("/session");
  await expect(app.getByTestId("session-new")).toHaveCount(0);
  await type(app, "111111");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  const section = app.getByTestId("session-section");
  await expect(section).toHaveCount(1);
  await expect(section.getByTestId("session-remove")).toBeVisible();
  const commands = await commandsSent(app);
  expect(commands).toContain("core_session_open");
  expect(commands).not.toContain("core_session_create");
});

test("an empty session goes when it is closed", async ({ app }) => {
  await app.goto("/session");
  await type(app, "246810");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.getByTestId("session-section").getByTestId("session-close").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_session_close");
});

test("a session can be deleted after asking once", async ({ app }) => {
  await app.goto("/session");
  await type(app, "246810");
  await expect(app).toHaveURL(/\/tabs\/chats$/);

  const section = app.getByTestId("session-section");
  await expect(section).toHaveCount(1);
  // Deleting asks once: the trash turns into a check, and only the check tells the core.
  await section.getByTestId("session-remove").click();
  expect(await commandsSent(app)).not.toContain("core_session_remove");
  await section.getByTestId("session-remove-sure").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_session_remove");
});

// 2026-10-01 (§108, QA SES-12): a session stays open until the user leaves it, also when the app
// starts again (the fake keeps its sessions across a reload, as the core keeps them on disk).
test("an open session is still there after the app starts again, with no pin", async ({ app }) => {
  await app.goto("/session");
  await type(app, "135790");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect(app.getByTestId("session-section")).toHaveCount(1);

  await app.reload();
  await expect(app.getByTestId("session-section")).toHaveCount(1);
  expect(await commandsSent(app)).not.toContain("core_session_open");
});

test("a session that was left is not there after the app starts again", async ({ app }) => {
  await app.goto("/session");
  await type(app, "135790");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.getByTestId("session-section").getByTestId("session-close").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);

  await app.reload();
  await expect(app.getByTestId("chat-row").first()).toBeVisible();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
});
