// The "Play" button of a game invitation, inside the bubble (seen on the Lenovo tablet, 2026-10-02:
// in Mono and dark, light grey on a white bubble of this phone's). Its words have to be readable in
// both bubbles, in every colour and appearance: measured, as WCAG does, not looked at. The label is
// 600 weight but not large text, so it needs 4.5:1 against what is behind it: the button's tint laid
// over the bubble, at the label's two ends (this phone's bubble is a gradient).
import type { Locator } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const INVITATION = "🎮 Shall we play Tic-tac-toe? https://flickertalk.com/games/tictactoe";

/**
 * Where the palette itself is under 4.5:1, whatever the button does: white on Ember's and Aurora's
 * light accents (this phone's bubble, its own text too, about 3:1) and those accents as text on white
 * (the other one's button). The owner's decision, not this button's; marked so the day it changes
 * the test says so.
 */
const PALETTE_UNDER_AA = new Set(["ember light mine", "ember light theirs", "aurora light mine", "aurora light theirs"]);

type Rgba = [number, number, number, number];

/** `rgb()`, `rgba()` or `color(srgb …)`, as computed styles write them, in 0–1 channels. */
function parse(css: string): Rgba {
  const numbers = css.match(/[\d.]+/g)!.map(Number);
  if (css.startsWith("color(")) return [numbers[0], numbers[1], numbers[2], numbers[3] ?? 1];
  return [numbers[0] / 255, numbers[1] / 255, numbers[2] / 255, numbers[3] ?? 1];
}

const over = ([r, g, b, a]: Rgba, [R, G, B]: Rgba): Rgba => [r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a), 1];
const between = (from: Rgba, to: Rgba, at: number): Rgba => from.map((one, index) => one + (to[index] - one) * at) as Rgba;

function luminance([r, g, b]: Rgba): number {
  const linear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

function contrast(one: Rgba, other: Rgba): number {
  const [light, dark] = [luminance(one), luminance(other)].sort((a, b) => b - a);
  return (light + 0.05) / (dark + 0.05);
}

/** The label's colour, the bubble's text colour, the tint, and the bubble under each end of the label. */
async function measure(play: Locator) {
  const seen = await play.evaluate((button) => {
    const bubble = button.closest(".ft-bubble")!;
    const label = button.querySelector(".ft-play-game__label")!;
    const native = button.shadowRoot!.querySelector(".button-native")!;
    const box = bubble.getBoundingClientRect();
    const words = label.getBoundingClientRect();
    // How far along the bubble's 135° gradient each end of the label is (CSS's gradient line).
    const along = (x: number, y: number) =>
      0.5 + ((x - box.left - box.width / 2) * Math.SQRT1_2 + (y - box.top - box.height / 2) * Math.SQRT1_2) / ((box.width + box.height) * Math.SQRT1_2);
    const middle = words.top + words.height / 2;
    return {
      label: getComputedStyle(label).color,
      text: getComputedStyle(bubble).color,
      tint: getComputedStyle(native).backgroundColor,
      solid: getComputedStyle(bubble).backgroundColor,
      stops: getComputedStyle(bubble).backgroundImage.match(/(?:rgba?|color)\([^)]*\)/g) ?? [],
      at: [along(words.left, middle), along(words.right, middle)],
    };
  });
  const under = seen.stops.length === 2 ? seen.at.map((at) => between(parse(seen.stops[0]), parse(seen.stops[1]), at)) : [parse(seen.solid)];
  const ratios = under.map((bubble) => {
    const behind = over(parse(seen.tint), bubble);
    return contrast(over(parse(seen.label), behind), behind);
  });
  return { ...seen, worst: Math.min(...ratios) };
}

for (const direction of ["ember", "aurora", "mono"]) {
  for (const appearance of ["light", "dark"]) {
    test.describe(`${direction}, ${appearance}`, () => {
      test.beforeEach(async ({ app }) => {
        await app.addInitScript(
          ([chosenDirection, chosenAppearance, invitation]) => {
            localStorage.setItem("ft-direction", chosenDirection);
            localStorage.setItem("ft-appearance", chosenAppearance);
            (window as unknown as Record<string, unknown>).__ftFakeBobSays = [invitation];
            (window as unknown as Record<string, unknown>).__ftFakeISaid = [invitation];
          },
          [direction, appearance, INVITATION],
        );
        await app.goto(`/chat/${BOB}`);
      });

      // The cause (2026-10-02): `--color: inherit` took `ion-content`'s `--color`, the page's text.
      test("Play in this phone's bubble has the bubble's text colour", async ({ app }) => {
        const play = app.locator(".ft-msg.is-mine [data-test='play-game']");
        await expect(play).toBeVisible();
        const seen = await measure(play);
        expect(seen.label).toBe(seen.text);
      });

      for (const side of ["mine", "theirs"]) {
        test(`Play reads in ${side === "mine" ? "this phone's" : "the other one's"} bubble`, async ({ app }) => {
          test.fail(PALETTE_UNDER_AA.has(`${direction} ${appearance} ${side}`), "the palette itself is under 4.5:1 here");
          const play = app.locator(`.ft-msg.is-${side} [data-test='play-game']`);
          await expect(play).toBeVisible();
          const seen = await measure(play);
          expect(seen.worst, `label ${seen.label} on ${seen.tint} over ${seen.stops.join(" → ") || seen.solid}`).toBeGreaterThanOrEqual(4.5);
        });
      }
    });
  }
}
