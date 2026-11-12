import { commandsSent, expect, frameAsks, servePluginFrames, test } from "./helpers";

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
  await section.getByTestId("session-remove").click();
  expect(await commandsSent(app)).not.toContain("core_session_remove");
  await section.getByTestId("session-remove-sure").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_session_remove");
});

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

async function boardInside(app: import("@playwright/test").Page) {
  await servePluginFrames(app);
  await app.goto("/session");
  await type(app, "777777");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.goto("/chat/ft_hidden1234567");
  await app.getByTestId("apps").click();
  await app.getByTestId("app-com.flickertalk.markdown").click();
  expect(await frameAsks(app, { type: "ft.recordSet", id: "q1", key: "board/1", value: "the secret board" })).toBe(true);
}

test("a plugin's record kept inside a session is there again when it is opened again", async ({ app }) => {
  await boardInside(app);
  await app.goto("/tabs/chats");
  await app.getByTestId("session-section").getByTestId("session-close").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_session_close");
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId("app-com.flickertalk.markdown").click();
  expect(await frameAsks(app, { type: "ft.recordGet", id: "q2", key: "board/1" })).toBeNull();

  await app.goto("/session");
  await type(app, "777777");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await app.goto("/chat/ft_hidden1234567");
  await app.getByTestId("apps").click();
  await app.getByTestId("app-com.flickertalk.markdown").click();
  expect(await frameAsks(app, { type: "ft.recordGet", id: "q3", key: "board/1" })).toBe("the secret board");
});

test("a plugin's record kept inside a session goes with the session", async ({ app }) => {
  await boardInside(app);
  await app.goto("/tabs/chats");
  const section = app.getByTestId("session-section");
  await section.getByTestId("session-remove").click();
  await section.getByTestId("session-remove-sure").click();
  await expect(app.getByTestId("session-section")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_session_remove");

  await app.goto("/session");
  await type(app, "777777");
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  expect(await app.evaluate(() => JSON.stringify((window as unknown as { __ftFake: { state: { pluginData: object } } }).__ftFake.state.pluginData))).toBe("{}");
});
