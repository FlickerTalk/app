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
