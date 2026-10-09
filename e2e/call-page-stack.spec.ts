// Seen on the phones (2026-10-09): leaving the call screen and coming back through the call bar,
// again and again, must never stack call screens. There is one call screen, however many times
// it is left and gone back to, and hanging up leaves none behind and no history to step through.
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const CALL = "/call/ft_bob123456789";
const CHAT = "/chat/ft_bob123456789";

type Fake = { back: () => boolean; listening: () => boolean };

/** Every call screen the outlet holds, shown or not. */
const callPages = (page: Page) => page.locator("ion-router-outlet > .ion-page:has(ion-content.ft-call)");
const path = (page: Page) => page.evaluate(() => location.pathname);

/** Chats → Bob's chat → a voice call, connected; the way a user gets there. */
async function callFromChats(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as { __ftFakeNative: boolean }).__ftFakeNative = true;
  });
  await page.goto("/tabs/chats");
  await page.locator('[data-test="chat-row"]').first().click();
  await expect.poll(() => path(page)).toBe(CHAT);
  await page.getByRole("button", { name: "Voice call", exact: true }).click();
  await expect(page.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  await expect.poll(() => path(page)).toBe(CALL);
}

/** Android's back button, once the app listens to it. */
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
    // Hanging up goes back to the chat, leaves no call screen, and the chat's back goes to Chats.
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
  // Call screen → chat → Chats; the call bar takes me back from there.
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
  // Nothing of the call is left behind Chats: it is the first page again, as it was.
  expect(await app.evaluate(() => (history.state as { back: string | null } | null)?.back ?? null)).toBeNull();
});

// Found while checking the above (2026-10-09): the call bar shows as soon as the call screen starts
// to go, and a tap on it before the screen had gone (its 300 ms transition) left a blank page: the
// router was on the call, and Ionic took away the call screen that was leaving.
for (const [how, leave] of [
  ["the on-screen button", (page: Page) => page.getByRole("button", { name: "Back to the chat" }).click()],
  ["Android's back button", pressBack],
] as const) {
  test(`the call bar tapped while the call screen leaves with ${how} goes back to it`, async ({ app }) => {
    await callFromChats(app);
    for (let round = 0; round < 3; round++) {
      await leave(app);
      // At once: the call screen is still on its way out.
      await app.getByRole("button", { name: "Back to the call" }).click();
      await expect.poll(() => path(app)).toBe(CALL);
      // Past the end of the transition that was taking the call screen away (300 ms).
      await app.waitForTimeout(700);
      await expect(callPages(app)).toHaveCount(1);
      await expect(callPages(app)).not.toHaveClass(/ion-page-hidden/);
      await expect(app.locator(".ft-call__state")).toBeVisible();
      await expect(app.getByTestId("call-bar")).toHaveCount(0);
    }
  });
}
