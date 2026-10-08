// Felt on the iPhone (2026-10-09): entering a chat and going back was slow, and the back button
// appeared late. Ionic's iOS transition lasts 540 ms and fades the entering header in, so the back
// button was only all there when it ended (≈600 ms to enter, ≈577 ms back, measured on the iPhone).
// The app's own transition (page-transition.ts) moves the whole page in 300 ms, the same on iOS and
// Android: the back button is there, opaque and on screen, almost at once. Measured with the CPU
// slowed down four times, as a phone is next to a desktop. Here, with Ionic's: 608 ms on iOS and
// 306 ms on Android for the back button, 583 and 238 ms for the chat to go; with the app's, about
// 60–120 ms and 320–350 ms. The bounds leave room for a slow CI machine and still fail Ionic's iOS one.
import { devices, type Page } from "@playwright/test";
import { expect, test } from "./helpers";

/** Once a tap lands, the time (ms) until `ready()` first holds on an animation frame. */
async function timeAfterTap(app: Page, tap: () => Promise<void>, ready: string): Promise<number> {
  await app.evaluate((source) => {
    const clock = { tapped: 0, ready: 0 };
    (window as unknown as Record<string, unknown>).__ftClock = clock;
    const holds = new Function(`return (${source})()`) as () => boolean;
    addEventListener("click", () => (clock.tapped = performance.now()), { capture: true, once: true });
    const frame = () => {
      if (clock.tapped && holds()) clock.ready = performance.now();
      else requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }, ready);
  await tap();
  const clock = () => app.evaluate(() => (window as unknown as { __ftClock: { tapped: number; ready: number } }).__ftClock);
  await expect.poll(async () => (await clock()).ready, { timeout: 5_000 }).toBeGreaterThan(0);
  const { tapped, ready: at } = await clock();
  return at - tapped;
}

// The chat's back button is all there: on screen whole, and nothing above it see-through.
const BACK_BUTTON_THERE = `() => {
  const button = document.querySelector(".ft-thread ion-back-button");
  if (!button) return false;
  const box = button.getBoundingClientRect();
  if (box.width === 0 || box.left < 0 || box.right > innerWidth) return false;
  for (let el = button; el; el = el.parentElement ?? el.getRootNode().host) {
    const style = getComputedStyle(el);
    if (Number(style.opacity) < 0.99 || style.visibility === "hidden" || style.display === "none") return false;
  }
  return true;
}`;

// The chat's page is off the screen: gone from the page, hidden, or slid out whole.
const CHAT_GONE = `() => {
  const page = document.querySelector(".ft-thread")?.closest(".ion-page");
  if (!page || getComputedStyle(page).display === "none" || page.classList.contains("ion-page-hidden")) return true;
  const box = page.getBoundingClientRect();
  return box.left >= innerWidth || box.right <= 0;
}`;

/** The chat's Ionic page; Ionic turns its taps off while it comes in. */
const chatPage = (app: Page) => app.locator("div.ion-page", { has: app.getByTestId("peer") });

async function enterAndLeave(app: Page) {
  await app.goto("/tabs/chats");
  const row = app.locator(".ft-row", { hasText: "see you at six" }).first();
  // Once, so the chat's code is loaded: what is measured is the transition, not the dev server.
  await row.click();
  await expect(app.locator(".ft-thread ion-back-button")).toBeVisible();
  await app.goBack();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect(app.locator(".ft-thread")).toHaveCount(0);

  const cdp = await app.context().newCDPSession(app);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 4 });

  const backButton = await timeAfterTap(app, () => row.click(), BACK_BUTTON_THERE);
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  // Ionic turns the page's taps on when the transition ends.
  await expect(chatPage(app)).not.toHaveCSS("pointer-events", "none");
  const gone = await timeAfterTap(app, () => app.locator(".ft-thread ion-back-button").click(), CHAT_GONE);
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await cdp.send("Emulation.setCPUThrottlingRate", { rate: 1 });
  return { backButton, gone };
}

for (const phone of [
  { name: "an iPhone", use: { userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 } }, mode: "ios" },
  { name: "an iPhone, in Arabic", use: { userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 }, locale: "ar" }, mode: "ios" },
  { name: "an Android phone", use: { viewport: { width: 360, height: 740 } }, mode: "md" },
]) {
  test.describe(`on ${phone.name}`, () => {
    test.use(phone.use);

    test("the back button is there within 200 ms of tapping a chat, and the chat gone within 450 ms of tapping back", async ({ app }) => {
      await app.goto("/tabs/chats");
      await expect(app.locator("html")).toHaveClass(new RegExp(`\\b${phone.mode}\\b`));
      const { backButton, gone } = await enterAndLeave(app);
      test.info().annotations.push({ type: "measured", description: `back button there after ${backButton.toFixed(0)} ms, chat gone after ${gone.toFixed(0)} ms` });
      expect(backButton, "ms until the back button is all there").toBeLessThan(200);
      expect(gone, "ms until the chat is off the screen").toBeLessThan(450);
    });
  });
}

/** Where a finger `distance` px in from the leading edge of the screen is. */
function fromEdge(app: Page, distance: number, rtl: boolean) {
  return rtl ? app.viewportSize()!.width - 5 - distance : 5 + distance;
}

/** Moves the finger from `from` to `distance` px in from the leading edge, in `steps` moves. */
async function dragTo(app: Page, from: number, distance: number, steps: number, rtl: boolean) {
  const to = fromEdge(app, distance, rtl);
  for (let step = 1; step <= steps; step++) await app.mouse.move(from + ((to - from) * step) / steps, 400);
  return to;
}

/** Puts a finger on the leading edge of the screen and drags it `distance` px across. */
async function dragFromEdge(app: Page, distance: number, steps: number, rtl: boolean) {
  const from = fromEdge(app, 0, rtl);
  await app.mouse.move(from, 400);
  await app.mouse.down();
  return dragTo(app, from, distance, steps, rtl);
}

async function openBobsChat(app: Page) {
  await app.goto("/tabs/chats");
  await app.locator(".ft-row", { hasText: "see you at six" }).first().click();
  await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
  await expect(chatPage(app)).not.toHaveCSS("pointer-events", "none");
  return chatPage(app);
}

// iOS's swipe back drives the transition step by step (Ionic's gesture, `swipeBackEnabled`).
for (const ios of [
  { name: "an iPhone", use: { userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 } }, rtl: false },
  { name: "an iPhone, in Arabic", use: { userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 }, locale: "ar" }, rtl: true },
]) {
  test.describe(`on ${ios.name}, swiping back`, () => {
    test.use(ios.use);

    test("a swipe from the edge goes back to the chats", async ({ app }) => {
      await openBobsChat(app);
      await dragFromEdge(app, 300, 10, ios.rtl);
      await app.mouse.up();
      await expect(app).toHaveURL(/\/tabs\/chats$/);
      await expect(app.locator(".ft-thread")).toHaveCount(0);
    });

    test("the chat follows the finger, and goes back in place when the swipe is undone", async ({ app }) => {
      const page = await openBobsChat(app);
      const at = await dragFromEdge(app, 150, 15, ios.rtl);
      // Mid-swipe the whole chat page has moved with the finger, toward the trailing side.
      await expect.poll(async () => (await page.boundingBox())!.x * (ios.rtl ? -1 : 1)).toBeGreaterThan(50);
      await dragTo(app, at, 20, 15, ios.rtl);
      await app.mouse.up();
      await expect.poll(async () => (await page.boundingBox())!.x).toBe(0);
      await expect(app).toHaveURL(/\/chat\/ft_bob123456789$/);
      await expect(app.locator(".ft-thread ion-back-button")).toBeVisible();
    });
  });
}

// A tab switch is not a push: it is not animated, as before.
test.describe("on a phone, switching tabs", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("shows the other tab at once, without sliding", async ({ app }) => {
    await app.goto("/tabs/chats");
    await expect(app.locator(".ft-row").first()).toBeVisible();
    await app.evaluate(() => {
      const seen: string[] = [];
      (window as unknown as Record<string, unknown>).__ftMoved = seen;
      const end = performance.now() + 600;
      const frame = () => {
        for (const page of document.querySelectorAll(".ion-page")) {
          const transform = getComputedStyle(page).transform;
          if (transform !== "none" && transform !== "matrix(1, 0, 0, 1, 0, 0)") seen.push(transform);
        }
        if (performance.now() < end) requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    });
    await app.getByRole("tab", { name: "Settings" }).click();
    await expect(app).toHaveURL(/\/tabs\/settings$/);
    await app.waitForTimeout(700);
    expect(await app.evaluate(() => (window as unknown as { __ftMoved: string[] }).__ftMoved)).toEqual([]);
  });
});

// Seen on the iPhone (2026-10-09, Ionic's iOS transition): going back, the Chats page's large title
// was drawn over the still chat page (over the contact's status, then under the name) for about
// 500 ms, then the screen cut to the list. Now the two pages slide as they are: nothing of the
// page coming in is drawn on its own over the one going.
test.describe("on an iPhone, going back to the chats", () => {
  test.use({ userAgent: devices["iPhone 13"].userAgent, viewport: { width: 375, height: 812 } });

  test("the chat slides out whole, and the large «Chats» title comes in with its page", async ({ app }) => {
    await openBobsChat(app);
    await app.evaluate(() => {
      const seen = { frames: 0, chatAt: [] as number[], clones: [] as string[], titleOutside: [] as string[] };
      (window as unknown as Record<string, unknown>).__ftBack = seen;
      const chat = document.querySelector(".ft-thread")!.closest(".ion-page")!;
      const end = performance.now() + 800;
      const frame = () => {
        seen.frames++;
        if (chat.isConnected && getComputedStyle(chat).display !== "none") seen.chatAt.push(chat.getBoundingClientRect().left);
        // Ionic's iOS transition draws copies of titles and back buttons over the pages.
        for (const clone of document.querySelectorAll(".ion-cloned-element")) {
          if (getComputedStyle(clone).display !== "none") seen.clones.push(clone.tagName);
        }
        const title = document.querySelector("ion-title[size='large']");
        const page = title?.closest(".ion-page");
        if (title && page && getComputedStyle(title).opacity !== "0") {
          const [t, p] = [title.getBoundingClientRect(), page.getBoundingClientRect()];
          if (t.left < p.left - 1 || t.right > p.right + 1) seen.titleOutside.push(`${t.left} vs page ${p.left}`);
        }
        if (performance.now() < end) requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    });
    await app.locator(".ft-thread ion-back-button").click();
    await expect(app).toHaveURL(/\/tabs\/chats$/);
    await app.waitForTimeout(900);
    const seen = await app.evaluate(() => (window as unknown as { __ftBack: { chatAt: number[]; clones: string[]; titleOutside: string[] } }).__ftBack);
    expect(seen.clones, "copies drawn over the pages").toEqual([]);
    expect(seen.titleOutside, "the large title away from its page").toEqual([]);
    // The chat moved across the screen, not stood still and then vanished.
    const between = seen.chatAt.filter((left) => left > 10 && left < 365);
    expect(between.length, `the chat's positions ${JSON.stringify(seen.chatAt)}`).toBeGreaterThanOrEqual(3);
  });
});

// Ioan (2026-10-09): «¿respetas toda la estructura tal como tiene que ser?». Ionic's transitions
// find what to move among a page's own children: `ion-header`, `ion-content` and `ion-footer` in a
// page, `ion-tabs` in the tabs' page. Wrapped in a box of the app's own, they were not found.
test.describe("on a phone, the pages are Ionic's own shape", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("the chat is header, content and footer, and the tabs' page holds its tabs", async ({ app }) => {
    await openBobsChat(app);
    const shape = await app.evaluate(() => {
      const chat = document.querySelector("[data-test='peer']")!.closest(".ion-page")!;
      const tabs = document.querySelector("ion-tab-bar")!.closest("ion-tabs")!;
      return {
        chat: [...chat.children].map((child) => child.tagName.toLowerCase()).filter((tag) => tag.startsWith("ion-") && tag !== "ion-modal"),
        tabsInPage: tabs.parentElement!.classList.contains("ion-page"),
      };
    });
    expect(shape.chat).toEqual(["ion-header", "ion-content", "ion-footer"]);
    expect(shape.tabsInPage).toBe(true);
  });
});
