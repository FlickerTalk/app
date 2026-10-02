// What someone wrote reads in its own direction, whatever the app's language (seen in Arabic on
// 2026-10-02: «¿Probamos la hoja de apps?» showed as «?Probamos la hoja de apps¿», the emoji at the
// other end). The app's own texts keep the app's direction; a name in a list stays where the app
// puts names, beside the avatar.
import type { Locator } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const SAID = "¿Probamos la hoja de apps? 🙂";
const LONG = "Mañana a las seis en la estación, ¿vale? Llevo yo los billetes y algo de comer para el viaje.";

/** Where, from the left of the screen, the character at `at` of the element's text is drawn. */
const xOf = (element: Locator, at: number) =>
  element.evaluate((node, index) => {
    const text = [...node.childNodes].find((one) => one.nodeType === Node.TEXT_NODE && one.textContent!.trim())!;
    const range = document.createRange();
    range.setStart(text, index);
    range.setEnd(text, index + 1);
    return range.getBoundingClientRect().x;
  }, at);

test.describe("in Arabic, right to left", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });

  test("a Spanish message is laid out left to right, its punctuation where it was written", async ({ app }) => {
    await app.addInitScript((said) => ((window as unknown as Record<string, unknown>).__ftFakeBobSays = said), [SAID, LONG]);
    await app.goto(`/chat/${BOB}`);
    await expect(app.locator("html")).toHaveAttribute("dir", "rtl");
    const text = app.locator(".ft-bubble__text", { hasText: "Probamos" });
    await expect(text).toHaveText(SAID);
    await expect(text).toHaveCSS("direction", "ltr");
    // «¿» first, at the left; «?» after «apps», further right; the emoji last, at the right.
    const opening = await xOf(text, 0);
    const closing = await xOf(text, SAID.indexOf("?"));
    const emoji = await xOf(text, SAID.indexOf("🙂"));
    expect(opening).toBeLessThan(closing);
    expect(closing).toBeLessThan(emoji);
    // Over several lines it is a left-to-right paragraph: every line starts at the bubble's left.
    const long = app.locator(".ft-bubble__text", { hasText: "Mañana" });
    const lines = await long.evaluate((node) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      const left = node.getBoundingClientRect().left + parseFloat(getComputedStyle(node).paddingLeft);
      // A line can be more than one box (the space where it wraps is one): its start is the leftmost.
      const starts = new Map<number, number>();
      for (const box of range.getClientRects()) starts.set(box.top, Math.min(starts.get(box.top) ?? Infinity, box.left));
      return [...starts.values()].map((start) => Math.round(start - left));
    });
    expect(lines.length).toBeGreaterThan(1);
    for (const start of lines) expect(start).toBe(0);
    // The app's own words around it stay right to left.
    await expect(app.locator(".ft-thread__bar .ft-peer__status")).toHaveCSS("direction", "rtl");
  });

  test("a Latin name in the list keeps to the avatar's side", async ({ app }) => {
    await app.goto("/tabs/chats");
    const row = app.getByTestId("chat-row").first();
    const name = row.locator(".ft-row__name");
    await expect(name).toHaveText("Bob");
    await expect(name).toHaveCSS("direction", "ltr");
    // Right to left, the avatar is at the right: the name's text ends where its box does.
    const box = (await name.boundingBox())!;
    const ends = await name.evaluate((node) => {
      const range = document.createRange();
      range.selectNodeContents(node);
      return range.getBoundingClientRect().right;
    });
    expect(box.x + box.width - ends).toBeLessThan(1);
  });
});
