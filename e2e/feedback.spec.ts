import type { Locator, Page } from "@playwright/test";
import { callsTo, commandsSent, expect, test } from "./helpers";

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

const pressBack = (app: Page) => app.evaluate(() => (window as unknown as { __ftFake: { back: () => boolean } }).__ftFake.back());

async function openModal(app: Page): Promise<Locator> {
  await app.getByTestId("feedback").click();
  const modal = app.getByTestId("feedback-modal");
  await expect(modal.getByTestId("send")).toBeInViewport({ ratio: 1 });
  return modal;
}

const box = (modal: Locator) => modal.locator("ion-textarea textarea");

const MODES = ["md", "ios"] as const;
const IN_SETTINGS = /\/tabs\/settings(\?|$)/;

function drawnSides(host: Element): string[] {
  const names = ["Top", "Right", "Bottom", "Left"] as const;
  const visible = (color: string) => !/rgba?\([^)]*,\s*0\)$|transparent/.test(color);
  const lines = [host, ...host.querySelectorAll("*")].flatMap((element) => {
    const style = getComputedStyle(element) as unknown as Record<string, string>;
    const drawn = names.filter(
      (name) => parseFloat(style[`border${name}Width`]) >= 1 && style[`border${name}Style`] !== "none" && visible(style[`border${name}Color`]),
    );
    const shape = element.getBoundingClientRect();
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

async function inSightAndUncovered(locator: Locator, bottom: number) {
  await expect(locator).toBeInViewport({ ratio: 1 });
  await expect
    .poll(() =>
      locator.evaluate((element, limit) => {
        const shape = element.getBoundingClientRect();
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

    await box(modal).press("End");
    await box(modal).pressSequentially("!");
    await expect(box(modal)).toHaveValue("ملصقات من فضلكم!");
    await expect(modal.getByTestId("outcome")).toHaveCount(0);

    await app.keyboard.press("Escape");
    await expect(modal.getByTestId("send")).toBeInViewport();
    await expect(box(modal)).toHaveValue("ملصقات من فضلكم!");

    expect(await pressBack(app)).toBe(true);
    await expect(modal).toBeHidden();
    await expect(app).toHaveURL(IN_SETTINGS);
    expect(await pressBack(app)).toBe(false);
    await openModal(app);
    await expect(box(modal)).toHaveValue("");
    await expect(modal.getByTestId("outcome")).toHaveCount(0);

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
    await modal.locator("ion-backdrop").click(outside);
    await expect(modal.getByTestId("send")).toBeInViewport();
    await expect(box(modal)).toHaveValue("A long idea I would hate to lose");

    await box(modal).fill("");
    await modal.locator("ion-backdrop").click(outside);
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
    await box(modal).fill("Stickers, please");
    await inSightAndUncovered(hint, 740);
    await inSightAndUncovered(send, 740);

    await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(300));
    await expect.poll(() => modal.evaluate((element) => element.getBoundingClientRect().bottom)).toBe(740 - 300);
    await inSightAndUncovered(send, 740 - 300);
    await inSightAndUncovered(hint, 740 - 300);
  });
});

test.describe("from the Apps tab, on a 360 × 740 phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  const KINDS = [
    { show: "games", title: "Suggest a game", line: "[Game idea]" },
    { show: "tools", title: "Suggest a tool", line: "[Tool idea]" },
  ] as const;

  for (const { show, title, line } of KINDS)
  test(`${title}: the idea goes with the line that says which, and keeps its text when it does not go`, async ({ app }) => {
    await app.goto(`/tabs/apps?show=${show}`);
    await app.getByTestId("apps-suggest").click();
    const modal = app.getByTestId("feedback-modal");
    await expect(modal.getByTestId("send")).toBeInViewport({ ratio: 1 });
    await expect(modal.locator("ion-title")).toHaveText(title);
    await expect(modal.getByTestId("hint")).toHaveText(
      "It reaches us without your name or any identifier. We cannot reply. Do not write personal data.",
    );
    await expect(modal.locator("ion-textarea .counter")).toHaveText("0 / 1988");
    await expect(modal.getByTestId("send")).toHaveAttribute("disabled", "");

    await box(modal).fill("  Something new  ");
    await modal.getByTestId("send").click();
    await expect(modal.getByTestId("outcome")).toHaveText("Could not send. Try again later");
    await expect(box(modal)).toHaveValue("  Something new  ");
    const sent = (await callsTo(app)).filter(([command]) => command === "core_send_feedback").map(([, args]) => args?.text);
    expect(sent).toEqual([`${line}\nSomething new`]);
    await expect(app).toHaveURL(new RegExp(`/tabs/apps\\?show=${show}$`));

    expect(await pressBack(app)).toBe(true);
    await expect(modal).toBeHidden();
    await expect(app).toHaveURL(new RegExp(`/tabs/apps\\?show=${show}$`));
  });

  test("the entry shows up once, in its place, and nothing moves it while the tab loads", async ({ app }) => {
    await app.addInitScript(() => {
      const tops: number[] = [];
      Object.assign(window, { __ftFakeCatalogueLate: 1500, __suggestTops: tops });
      const watch = () => {
        const entry = document.querySelector("[data-test='apps-suggest']");
        const top = entry ? Math.round(entry.getBoundingClientRect().top) : null;
        if (top !== null && top !== tops[tops.length - 1]) tops.push(top);
        requestAnimationFrame(watch);
      };
      requestAnimationFrame(watch);
    });
    await app.goto("/tabs/apps?show=games");
    await expect(app.getByTestId("apps-installed")).toBeVisible();
    await expect(app.getByTestId("apps-suggest")).toHaveCount(0);

    await expect(app.getByTestId("apps-more")).toBeVisible();
    await expect(app.getByTestId("apps-suggest")).toBeVisible();
    await app.evaluate(() => document.fonts.ready);
    await app.evaluate(() => Promise.all([...document.images].map((image) => image.decode().catch(() => undefined))));
    await app.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    expect(await app.evaluate(() => (window as unknown as { __suggestTops: number[] }).__suggestTops)).toHaveLength(1);
  });
});

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

test.describe("the text box on iOS", () => {
  test.use({ viewport: { width: 390, height: 780 } });

  test("draws its border in Ionic's medium colour", async ({ app }) => {
    await app.goto("/tabs/settings?ionic:mode=ios");
    const modal = await openModal(app);
    const colours = await modal.locator("ion-textarea").evaluate((host) => {
      const probe = document.createElement("span");
      probe.style.color = "var(--ion-color-medium)";
      host.parentElement!.appendChild(probe);
      const medium = getComputedStyle(probe).color;
      probe.remove();
      const style = getComputedStyle(host);
      return { medium, sides: [style.borderTopColor, style.borderRightColor, style.borderBottomColor, style.borderLeftColor] };
    });
    expect(colours.sides).toEqual([colours.medium, colours.medium, colours.medium, colours.medium]);
  });
});
