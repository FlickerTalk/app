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

// The bubble mirrors with the language (decided 2026-10-02): this phone's messages sit at the end
// of the line and the other one's at the start, in English and in Arabic alike, and the tail (the
// one small corner), the badges over a picture and a voice message's clock go with them, to the
// outer side. Measured per physical corner and edge.
const FILES = [
  { outgoing: true, name: "beach.jpg", mime: "image/jpeg", state: "done" },
  { outgoing: false, name: "lost.jpg", mime: "image/jpeg", state: "failed" },
  { outgoing: true, name: "note.m4a", mime: "audio/mp4", state: "done" },
];

for (const [language, rtl] of [["en", false], ["ar", true]] as const) {
  test.describe(`the bubbles in ${language === "en" ? "English" : "Arabic"}`, () => {
    test.use({ viewport: { width: 360, height: 740 }, locale: language });

    test("this phone's at the end, the other one's at the start, each with its tail outside", async ({ app }) => {
      await app.addInitScript((files) => {
        (window as unknown as Record<string, unknown>).__ftFakeISaid = ["on my way"];
        (window as unknown as Record<string, unknown>).__ftFakeFiles = files;
      }, FILES);
      await app.goto(`/chat/${BOB}`);
      const screen = 360;
      // The outer side: right for this phone's in English, left in Arabic; the other way for theirs.
      const outer = (mine: boolean) => (mine !== rtl ? "Right" : "Left");
      const inner = (mine: boolean) => (mine !== rtl ? "Left" : "Right");
      const corners = (locator: import("@playwright/test").Locator) =>
        locator.evaluate((node) => ({ Left: getComputedStyle(node).borderBottomLeftRadius, Right: getComputedStyle(node).borderBottomRightRadius }));

      for (const mine of [true, false]) {
        const bubble = app.locator(`.ft-msg.is-${mine ? "mine" : "theirs"} .ft-bubble:not(.is-media):not(.is-voice)`).last();
        await expect(bubble).toBeVisible();
        const box = (await bubble.boundingBox())!;
        expect(box.x + box.width / 2 > screen / 2, "which half of the screen").toBe(outer(mine) === "Right");
        const radii = await corners(bubble);
        expect(radii[outer(mine)], "the tail, outside").toBe("6px");
        expect(radii[inner(mine)], "the inner corner, round").not.toBe("6px");
      }

      // A picture of this phone's: its tail outside, its clock at the outer bottom corner and the
      // save button at the outer top one, 10 and 8 px in.
      const picture = app.locator(".ft-msg.is-mine .ft-media").last();
      await expect(picture).toBeVisible();
      expect((await corners(picture))[outer(true)]).toBe("6px");
      const frame = (await picture.boundingBox())!;
      const gap = async (selector: string, side: "Left" | "Right") => {
        const one = (await app.locator(`.ft-msg.is-mine ${selector}`).last().boundingBox())!;
        return Math.round(side === "Left" ? one.x - frame.x : frame.x + frame.width - (one.x + one.width));
      };
      expect(await gap(".ft-bubble.is-media .ft-bubble__meta", outer(true))).toBe(10);
      expect(await gap(".ft-media__save", outer(true))).toBe(8);

      // A picture of theirs that failed: «Failed» at its outer side, the start of the line.
      const lost = app.locator(".ft-msg.is-theirs .ft-media").last();
      const lostFrame = (await lost.boundingBox())!;
      const failed = (await app.locator(".ft-msg.is-theirs .ft-media__failed").last().boundingBox())!;
      expect(Math.round(outer(false) === "Left" ? failed.x - lostFrame.x : lostFrame.x + lostFrame.width - (failed.x + failed.width))).toBe(10);

      // A voice message's clock keeps to the end of its slot.
      const clock = app.locator(".ft-msg.is-mine .ft-voice__meta").last();
      await expect(clock).toBeVisible();
      const slack = await clock.evaluate((node, right) => {
        const range = document.createRange();
        range.selectNodeContents(node);
        const text = range.getBoundingClientRect();
        const slot = node.getBoundingClientRect();
        return Math.round(right ? slot.right - text.right : text.left - slot.left);
      }, !rtl);
      expect(slack).toBe(0);
    });

    // What the «+» unfolds rises over it from the same side, inside the screen.
    test("the attach options unfold over the «+», on the screen", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await app.locator(".ft-attach > button").click();
      const plus = (await app.locator(".ft-attach > button").boundingBox())!;
      const option = app.locator(".ft-attach__list ion-fab-button").first();
      await expect(option).toBeVisible();
      const list = (await app.locator(".ft-attach__list").boundingBox())!;
      expect(list.x).toBeGreaterThanOrEqual(0);
      expect(list.x + list.width).toBeLessThanOrEqual(360);
      // Shifted from the «+» towards the middle of the screen, by the same amount either way.
      const shift = (await option.boundingBox())!.x + 20 - (plus.x + plus.width / 2);
      expect(rtl ? -shift : shift).toBe(6);
    });
  });
}
