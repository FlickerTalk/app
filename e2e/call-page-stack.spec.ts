import type { Page } from "@playwright/test";
import { expect, servePluginFrames, test } from "./helpers";

const CALL = "/call/ft_bob123456789";
const CHAT = "/chat/ft_bob123456789";

const MARKDOWN = "com.flickertalk.markdown";
const SKETCH = "com.flickertalk.sketch";

type Fake = {
  back: () => boolean;
  listening: () => boolean;
  tapReminder: (plugin: string, id: string) => void;
  theyAnswer: () => void;
  ring: () => void;
};

const callPages = (page: Page) => page.locator("ion-router-outlet > .ion-page:has(ion-content.ft-call)");
const path = (page: Page) => page.evaluate(() => location.pathname);

async function callFromChats(page: Page, { answered = true } = {}) {
  await page.addInitScript((later) => {
    Object.assign(window, { __ftFakeNative: true, __ftFakeAnswerLater: later });
  }, !answered);
  await page.goto("/tabs/chats");
  await page.locator('[data-test="chat-row"]').first().click();
  await expect.poll(() => path(page)).toBe(CHAT);
  await page.getByRole("button", { name: "Voice call", exact: true }).click();
  await expect(page.locator(".ft-call__state")).toHaveText(answered ? /^00:0\d$/ : "Calling…");
  await expect.poll(() => path(page)).toBe(CALL);
}

async function tapReminder(page: Page, plugin: string) {
  await page.evaluate((id) => (window as unknown as { __ftFake: Fake }).__ftFake.tapReminder(id, "r1"), plugin);
  await expect.poll(() => path(page)).toBe(`/plugin/${plugin}`);
}

async function pressBack(page: Page) {
  await expect.poll(() => page.evaluate(() => (window as unknown as { __ftFake: Fake }).__ftFake.listening())).toBe(true);
  await page.evaluate(() => (window as unknown as { __ftFake: Fake }).__ftFake.back());
}

async function backToTheCall(page: Page) {
  await page.getByRole("button", { name: "Back to the call" }).click();
  await expect.poll(() => path(page)).toBe(CALL);
  await expect(page.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
}

for (const [how, leave] of [
  ["the on-screen button", (page: Page) => page.getByRole("button", { name: "Back to the chat" }).click()],
  ["Android's back button", pressBack],
] as const) {
  test(`leaving the call screen with ${how} and coming back, three times, keeps one call screen`, async ({ app }) => {
    await callFromChats(app);
    for (let round = 0; round < 3; round++) {
      await leave(app);
      await expect.poll(() => path(app)).toBe(CHAT);
      await expect(app.getByTestId("call-bar")).toBeVisible();
      await expect(callPages(app)).toHaveCount(0);
      await backToTheCall(app);
      await expect(callPages(app)).toHaveCount(1);
    }
    await app.locator(".ft-call").getByRole("button", { name: "Hang up" }).click();
    await expect.poll(() => path(app)).toBe(CHAT);
    await expect(callPages(app)).toHaveCount(0);
    await app.locator("ion-back-button:visible").click();
    await expect.poll(() => path(app)).toBe("/tabs/chats");
    await expect(callPages(app)).toHaveCount(0);
  });
}

test("going back to the call from Chats, three times, keeps one call screen and no history", async ({ app }) => {
  await callFromChats(app);
  await app.getByRole("button", { name: "Back to the chat" }).click();
  await expect.poll(() => path(app)).toBe(CHAT);
  await app.locator("ion-back-button:visible").click();
  await expect.poll(() => path(app)).toBe("/tabs/chats");
  for (let round = 0; round < 3; round++) {
    await backToTheCall(app);
    await expect(callPages(app)).toHaveCount(1);
    await app.getByRole("button", { name: "Back to the chat" }).click();
    await expect.poll(() => path(app)).toBe("/tabs/chats");
    await expect(callPages(app)).toHaveCount(0);
  }
  await backToTheCall(app);
  await app.locator(".ft-call").getByRole("button", { name: "Hang up" }).click();
  await expect.poll(() => path(app)).toBe("/tabs/chats");
  await expect(callPages(app)).toHaveCount(0);
  expect(await app.evaluate(() => (history.state as { back: string | null } | null)?.back ?? null)).toBeNull();
});

for (const [how, leave] of [
  ["the on-screen button", (page: Page) => page.getByRole("button", { name: "Back to the chat" }).click()],
  ["Android's back button", pressBack],
] as const) {
  test(`the call bar tapped while the call screen leaves with ${how} goes back to it`, async ({ app }) => {
    await callFromChats(app);
    for (let round = 0; round < 3; round++) {
      await leave(app);
      await app.getByRole("button", { name: "Back to the call" }).click();
      await expect.poll(() => path(app)).toBe(CALL);
      await app.waitForTimeout(700);
      await expect(callPages(app)).toHaveCount(1);
      await expect(callPages(app)).not.toHaveClass(/ion-page-hidden/);
      await expect(app.locator(".ft-call__state")).toBeVisible();
      await expect(app.getByTestId("call-bar")).toHaveCount(0);
    }
  });
}

for (const [how, over] of [
  ["one page", [MARKDOWN]],
  ["two pages", [MARKDOWN, SKETCH]],
] as const) {
  test(`the call bar on ${how} opened over the call screen goes back to that call screen`, async ({ app }) => {
    await servePluginFrames(app);
    await callFromChats(app);
    for (const [at, plugin] of over.entries()) {
      if (at) await app.waitForTimeout(400);
      await tapReminder(app, plugin);
    }
    await expect(app.getByTestId("call-bar")).toBeVisible();
    await expect(callPages(app)).toHaveCount(1);
    await backToTheCall(app);
    await app.waitForTimeout(700);
    await expect(callPages(app)).toHaveCount(1);
    await expect(callPages(app)).not.toHaveClass(/ion-page-hidden/);
    await expect(app.locator("ion-router-outlet > .ion-page:has(.ft-plugin__frame)")).toHaveCount(0);
    await app.getByRole("button", { name: "Back to the chat" }).click();
    await expect.poll(() => path(app)).toBe(CHAT);
    await expect(callPages(app)).toHaveCount(0);
    await app.locator("ion-back-button:visible").click();
    await expect.poll(() => path(app)).toBe("/tabs/chats");
  });
}

test("a call answered while a page is open over its call screen goes back to that call screen", async ({ app }) => {
  await servePluginFrames(app);
  await callFromChats(app, { answered: false });
  await tapReminder(app, MARKDOWN);
  await expect(callPages(app)).toHaveCount(1);
  await app.evaluate(() => (window as unknown as { __ftFake: Fake }).__ftFake.theyAnswer());
  await expect.poll(() => path(app)).toBe(CALL);
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  await app.waitForTimeout(700);
  await expect(callPages(app)).toHaveCount(1);
  await expect(callPages(app)).not.toHaveClass(/ion-page-hidden/);
  await expect(app.locator("ion-router-outlet > .ion-page:has(.ft-plugin__frame)")).toHaveCount(0);
  await app.getByRole("button", { name: "Back to the chat" }).click();
  await expect.poll(() => path(app)).toBe(CHAT);
  await expect(callPages(app)).toHaveCount(0);
  await app.locator("ion-back-button:visible").click();
  await expect.poll(() => path(app)).toBe("/tabs/chats");
});

test("a call answered with the last call's screen still under the page on top goes back to that screen", async ({ app }) => {
  await servePluginFrames(app);
  await callFromChats(app);
  await tapReminder(app, MARKDOWN);
  await app.getByTestId("call-bar").getByRole("button", { name: "Hang up" }).click();
  await expect(app.getByTestId("call-bar")).toHaveCount(0);
  await expect(callPages(app)).toHaveCount(1);
  await app.evaluate(() => (window as unknown as { __ftFake: Fake }).__ftFake.ring());
  await app.getByTestId("incoming").getByRole("button", { name: "Answer" }).click();
  await expect.poll(() => path(app)).toBe(CALL);
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  await app.waitForTimeout(700);
  await expect(callPages(app)).toHaveCount(1);
  await expect(callPages(app)).not.toHaveClass(/ion-page-hidden/);
  await app.getByRole("button", { name: "Back to the chat" }).click();
  await expect.poll(() => path(app)).toBe(CHAT);
  await expect(callPages(app)).toHaveCount(0);
});

test("a page opened over the call screen while it still comes in stays when the call ends", async ({ app }) => {
  await servePluginFrames(app);
  await app.addInitScript(() => Object.assign(window, { __ftFakeNative: true, __ftFakeAnswerLater: true }));
  await app.goto("/tabs/chats");
  await app.locator('[data-test="chat-row"]').first().click();
  await expect.poll(() => path(app)).toBe(CHAT);
  await app.evaluate((plugin) => {
    const tap = () => {
      const screen = document.querySelector("ion-router-outlet > .ion-page:has(ion-content.ft-call)");
      const moving = screen?.getAnimations().filter((animation) => animation.playState === "running") ?? [];
      if (!moving.length) return void requestAnimationFrame(tap);
      moving.forEach((animation) => animation.pause());
      (window as unknown as { __ftFake: Fake }).__ftFake.tapReminder(plugin, "r1");
      const resume = () => {
        const tool = document.querySelector("ion-router-outlet > .ion-page:has(ion-content.ft-plugin-page)");
        if (!tool) return void requestAnimationFrame(resume);
        moving.forEach((animation) => animation.play());
      };
      requestAnimationFrame(resume);
    };
    requestAnimationFrame(tap);
  }, MARKDOWN);
  await app.getByRole("button", { name: "Voice call", exact: true }).click();
  await expect.poll(() => path(app)).toBe(`/plugin/${MARKDOWN}`);
  await expect(callPages(app)).toHaveClass(/ion-page-hidden/);
  await app.getByTestId("call-bar").getByRole("button", { name: "Hang up" }).click();
  await expect(app.getByTestId("call-bar")).toHaveCount(0);
  await app.waitForTimeout(2500);
  expect(await path(app)).toBe(`/plugin/${MARKDOWN}`);
  await expect(app.locator("ion-router-outlet > .ion-page:has(.ft-plugin__frame)")).not.toHaveClass(/ion-page-hidden/);
});
