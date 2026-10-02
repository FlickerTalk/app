// A suggestion from Settings (2026-10-02), in a modal over Settings (Ioan), on a phone. In Arabic the
// modal reads right to left, the counter keeps its digits in order, and a send the core does not
// confirm (here the fake core has no such command, like an app talking to a router without the
// endpoint) never says "sent" and keeps what was written. Every check retries: the app sets the
// direction and draws the modal a moment after the page loads.
import type { Locator, Page } from "@playwright/test";
import { commandsSent, expect, test } from "./helpers";

/** Before the app loads: a visual viewport a test can shrink, as a phone's keyboard does. */
function installKeyboard() {
  const real = window.visualViewport!;
  let keyboard = 0;
  const fake = new EventTarget();
  Object.defineProperties(fake, {
    height: { get: () => (keyboard ? window.innerHeight - keyboard : real.height) },
    offsetTop: { get: () => 0 },
    width: { get: () => real.width },
    offsetLeft: { get: () => 0 },
    pageTop: { get: () => 0 },
    pageLeft: { get: () => 0 },
    scale: { get: () => 1 },
  });
  real.addEventListener("resize", () => fake.dispatchEvent(new Event("resize")));
  Object.defineProperty(window, "visualViewport", { value: fake, configurable: true });
  (window as unknown as { __keyboard: (height: number) => void }).__keyboard = (height) => {
    keyboard = height;
    fake.dispatchEvent(new Event("resize"));
  };
}

async function openModal(app: Page): Promise<Locator> {
  await app.goto("/tabs/settings");
  await app.getByTestId("feedback").click();
  const modal = app.getByTestId("feedback-modal");
  await expect(modal).toBeVisible();
  return modal;
}

/** Wholly within what can be seen (`bottom`, the top of the keyboard), and nothing drawn over it. */
async function inSightAndUncovered(locator: Locator, bottom: number) {
  await expect(locator).toBeInViewport({ ratio: 1 });
  await expect.poll(async () => {
    const box = (await locator.boundingBox())!;
    return box.y + box.height <= bottom;
  }).toBe(true);
  const box = (await locator.boundingBox())!;
  const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  const onTop = await locator.evaluate((element, point) => {
    const hit = document.elementFromPoint(point.x, point.y);
    return hit !== null && (element === hit || element.contains(hit));
  }, centre);
  expect(onTop).toBe(true);
}

test.describe("in Arabic, right to left", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });

  test("a suggestion that does not go keeps its text", async ({ app }) => {
    const modal = await openModal(app);
    await expect(app.locator("html")).toHaveAttribute("dir", "rtl");
    await expect(modal.getByTestId("hint")).toHaveText("يصلنا اقتراحك دون اسمك أو أي معرّف. لا يمكننا الرد عليك. لا تكتب بيانات شخصية.");
    const counter = modal.getByTestId("counter");
    await expect(counter).toHaveText("0 / 2000");
    await expect(counter).toHaveCSS("direction", "ltr");
    await expect(modal.getByTestId("send")).toBeDisabled();

    await modal.locator("ion-textarea textarea").fill("ملصقات من فضلكم");
    await expect(counter).toHaveText("15 / 2000");
    await modal.getByTestId("send").click();
    await expect(modal.getByTestId("outcome")).toHaveText("تعذّر الإرسال. حاول لاحقًا");
    await expect(modal.locator("ion-textarea textarea")).toHaveValue("ملصقات من فضلكم");
    await expect.poll(() => commandsSent(app)).toContain("core_send_feedback");
    // Settings stays where it was, under the modal.
    await expect(app).toHaveURL(/\/tabs\/settings$/);

    // Closing forgets it: opened again, the box is empty and nothing is said.
    await modal.getByTestId("feedback-close").click();
    await expect(modal).toHaveCount(0);
    await app.getByTestId("feedback").click();
    await expect(app.locator("[data-test='feedback-modal'] ion-textarea textarea")).toHaveValue("");
    await expect(app.getByTestId("outcome")).toHaveCount(0);
  });
});

test.describe("on a 360 × 740 phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test.beforeEach(async ({ app }) => {
    await app.addInitScript(installKeyboard);
  });

  test("the notice and the send button are in sight and not covered, also with the keyboard open", async ({ app }) => {
    const modal = await openModal(app);
    const hint = modal.getByTestId("hint");
    const send = modal.getByTestId("send");
    await inSightAndUncovered(hint, 740);
    await inSightAndUncovered(send, 740);

    // A phone's keyboard takes about 300 px: the modal sits on it and the button stays in sight.
    await modal.locator("ion-textarea textarea").fill("Stickers, please");
    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(300));
    await inSightAndUncovered(send, 740 - 300);
    await inSightAndUncovered(hint, 740 - 300);
  });
});
