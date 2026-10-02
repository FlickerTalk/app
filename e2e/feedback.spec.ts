// A suggestion from Settings (2026-10-02), in an Ionic modal over Settings (Ioan), on a phone. In
// Arabic the modal reads right to left, Ionic's counter keeps its digits in order, and a send the
// core does not confirm (here the fake core has no such command, like an app talking to a router
// without the endpoint) never says "sent" and keeps what was written. Every check retries: the app
// sets the direction, and Ionic presents the modal, a moment after the page loads.
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

/** Android's back button, pressed; whether the app was listening for it. */
const pressBack = (app: Page) => app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());

/** Opens the modal from Settings; Ionic moves it into `ion-app` and animates it in. */
async function openModal(app: Page): Promise<Locator> {
  await app.getByTestId("feedback").click();
  const modal = app.getByTestId("feedback-modal");
  await expect(modal.getByTestId("send")).toBeInViewport({ ratio: 1 });
  return modal;
}

const box = (modal: Locator) => modal.locator("ion-textarea textarea");

/** Ionic's two looks: Android's (`md`) and the iPhone's (`ios`), chosen here by the URL. */
const MODES = ["md", "ios"] as const;
const IN_SETTINGS = /\/tabs\/settings(\?|$)/;

/**
 * Which sides of the text box have a line drawn on them. The box is the outermost rectangle drawn
 * by any border inside the field (Ionic's `md` outline is three pieces; the counter sits outside
 * it), and a side counts when some border runs along that edge of it.
 */
function drawnSides(host: Element): string[] {
  const names = ["Top", "Right", "Bottom", "Left"] as const;
  const visible = (color: string) => !/rgba?\([^)]*,\s*0\)$|transparent/.test(color);
  const lines = [host, ...host.querySelectorAll("*")].flatMap((element) => {
    const style = getComputedStyle(element) as unknown as Record<string, string>;
    const drawn = names.filter(
      (name) => parseFloat(style[`border${name}Width`]) >= 1 && style[`border${name}Style`] !== "none" && visible(style[`border${name}Color`]),
    );
    const shape = element.getBoundingClientRect();
    // Something not laid out (Ionic's hidden notch) draws nothing.
    return drawn.length && shape.width > 0 && shape.height > 0 ? [{ shape, drawn }] : [];
  });
  if (!lines.length) return [];
  const edge = {
    Top: Math.min(...lines.map(({ shape }) => shape.top)),
    Right: Math.max(...lines.map(({ shape }) => shape.right)),
    Bottom: Math.max(...lines.map(({ shape }) => shape.bottom)),
    Left: Math.min(...lines.map(({ shape }) => shape.left)),
  };
  const at = (shape: DOMRect, name: (typeof names)[number]) => shape[name.toLowerCase() as "top" | "right" | "bottom" | "left"];
  return names
    .filter((name) => lines.some(({ shape, drawn }) => drawn.includes(name) && Math.abs(at(shape, name) - edge[name]) <= 2))
    .map((name) => name.toLowerCase());
}

/**
 * Wholly within what can be seen (`bottom`, the top of the keyboard), and nothing drawn over it.
 * Polled as a whole: Ionic slides the modal in, and a box measured mid-animation is stale.
 */
async function inSightAndUncovered(locator: Locator, bottom: number) {
  await expect(locator).toBeInViewport({ ratio: 1 });
  await expect
    .poll(() =>
      locator.evaluate((element, limit) => {
        const shape = element.getBoundingClientRect();
        // `elementFromPoint` stops at a shadow host (an `ion-button`'s inner button is in its shadow).
        const hit = document.elementFromPoint(shape.x + shape.width / 2, shape.y + shape.height / 2);
        return shape.bottom <= limit && hit !== null && (element === hit || element.contains(hit));
      }, bottom),
    )
    .toBe(true);
}

test.describe("in Arabic, right to left", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });

  for (const mode of MODES)
  test(`a suggestion that does not go keeps its text (${mode})`, async ({ app }) => {
    await app.goto(`/tabs/settings?ionic:mode=${mode}`);
    const modal = await openModal(app);
    await expect(app.locator("html")).toHaveAttribute("dir", "rtl");
    await expect(modal.getByTestId("hint")).toHaveText("يصلنا اقتراحك دون اسمك أو أي معرّف. لا يمكننا الرد عليك. لا تكتب بيانات شخصية.");
    const counter = modal.locator("ion-textarea .counter");
    await expect(counter).toHaveText("0 / 2000");
    await expect(modal.getByTestId("send")).toHaveAttribute("disabled", "");

    await box(modal).fill("ملصقات من فضلكم");
    await expect(counter).toHaveText("15 / 2000");
    // Drawn in that order too: the "1" left of the last "0", as in any language.
    const order = await counter.evaluate((element) => {
      const text = element.firstChild!;
      const range = document.createRange();
      const x = (at: number) => {
        range.setStart(text, at);
        range.setEnd(text, at + 1);
        return range.getBoundingClientRect().x;
      };
      return x(0) < x(text.textContent!.length - 1);
    });
    expect(order).toBe(true);

    await modal.getByTestId("send").click();
    await expect(modal.getByTestId("outcome")).toHaveText("تعذّر الإرسال. حاول لاحقًا");
    await expect(box(modal)).toHaveValue("ملصقات من فضلكم");
    await expect.poll(() => commandsSent(app)).toContain("core_send_feedback");
    await expect(app).toHaveURL(IN_SETTINGS);

    // What came of the send stays until the kept text is edited, then goes.
    await box(modal).press("End");
    await box(modal).pressSequentially("!");
    await expect(box(modal)).toHaveValue("ملصقات من فضلكم!");
    await expect(modal.getByTestId("outcome")).toHaveCount(0);

    // Escape is Ionic's "outside": with something written it does nothing.
    await app.keyboard.press("Escape");
    await expect(modal.getByTestId("send")).toBeInViewport();
    await expect(box(modal)).toHaveValue("ملصقات من فضلكم!");

    // Back closes it on purpose, once, and stays in Settings; opened again, it is empty and silent.
    expect(await pressBack(app)).toBe(true);
    await expect(modal).toBeHidden();
    await expect(app).toHaveURL(IN_SETTINGS);
    expect(await pressBack(app)).toBe(false);
    await openModal(app);
    await expect(box(modal)).toHaveValue("");
    await expect(modal.getByTestId("outcome")).toHaveCount(0);

    // The ✕ closes it whatever is written.
    await box(modal).fill("ملصقات");
    await modal.getByTestId("feedback-close").click();
    await expect(modal).toBeHidden();
  });
});

test.describe("on a tablet, where the modal floats over Settings", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("a tap outside never loses what was written, and closes it when nothing is", async ({ app }) => {
    await app.goto("/tabs/settings");
    const modal = await openModal(app);
    const outside = { position: { x: 20, y: 20 } };
    await box(modal).fill("A long idea I would hate to lose");
    await app.locator("ion-modal ion-backdrop").click(outside);
    await expect(modal.getByTestId("send")).toBeInViewport();
    await expect(box(modal)).toHaveValue("A long idea I would hate to lose");

    await box(modal).fill("");
    await app.locator("ion-modal ion-backdrop").click(outside);
    await expect(modal).toBeHidden();
  });
});

test.describe("on a 360 × 740 phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test.beforeEach(async ({ app }) => {
    await app.addInitScript(installKeyboard);
  });

  for (const mode of MODES)
  test(`the notice and the send button are in sight and not covered, also with the keyboard open (${mode})`, async ({ app }) => {
    await app.goto(`/tabs/settings?ionic:mode=${mode}`);
    const modal = await openModal(app);
    const hint = modal.getByTestId("hint");
    const send = modal.getByTestId("send");
    // Something written: a disabled button takes no taps, so nothing would be found on top of it.
    await box(modal).fill("Stickers, please");
    await inSightAndUncovered(hint, 740);
    await inSightAndUncovered(send, 740);

    // A phone's keyboard takes about 300 px: the modal shrinks to what is left and the button stays
    // in sight.
    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(300));
    await expect.poll(() => modal.evaluate((element) => element.getBoundingClientRect().bottom)).toBe(740 - 300);
    await inSightAndUncovered(send, 740 - 300);
    await inSightAndUncovered(hint, 740 - 300);
  });
});

// Found on the iOS simulator (2026-10-02): in Ionic's `ios` look `fill="outline"` draws nothing and
// the box was bare text on the page. It is a box in both looks, on all four sides.
test.describe("the text box", () => {
  test.use({ viewport: { width: 390, height: 780 } });

  for (const mode of MODES)
  test(`is drawn as a box on all four sides (${mode})`, async ({ app }) => {
    await app.goto(`/tabs/settings?ionic:mode=${mode}`);
    const modal = await openModal(app);
    const textarea = modal.locator("ion-textarea");
    await expect(textarea).toHaveClass(new RegExp(`\\b${mode}\\b`));
    await expect.poll(() => textarea.evaluate(drawnSides)).toEqual(["top", "right", "bottom", "left"]);
  });
});
