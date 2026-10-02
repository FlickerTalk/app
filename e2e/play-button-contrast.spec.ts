// The "Play" button of a game invitation, inside the bubble (seen on the Lenovo tablet, 2026-10-02:
// in Mono and dark, light grey on a white bubble of this phone's). It is Ionic's button in one of
// Ionic's named colours (the owner's rule, 2026-10-02): its words read on it, 4.5:1 (600 weight, not
// large text), and it reads as a button on the bubble, 3:1 against every point of the bubble behind
// it (this phone's bubble is a gradient). Measured, as WCAG does, not looked at.
import type { Locator } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const INVITATION = "🎮 Shall we play Tic-tac-toe? https://flickertalk.com/games/tictactoe";

/**
 * Where the light palettes of Ember and Aurora are themselves under the line: their accent is about
 * 3:1 against white and its own white text, so neither the accent button on the other one's bubble
 * nor a white button on this phone's Aurora bubble can reach it. Retouched on the branch
 * light-theme-contrast, which removes these marks.
 */
const PALETTE_UNDER = new Set(["ember light theirs", "aurora light theirs", "aurora light mine"]);

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

/** The label, the button under it and every point of the bubble under the button. */
async function measure(play: Locator) {
  const seen = await play.evaluate((button) => {
    const bubble = button.closest(".ft-bubble")!;
    return {
      label: getComputedStyle(button.querySelector(".ft-play-game__label")!).color,
      button: getComputedStyle(button.shadowRoot!.querySelector(".button-native")!).backgroundColor,
      solid: getComputedStyle(bubble).backgroundColor,
      stops: getComputedStyle(bubble).backgroundImage.match(/(?:rgba?|color)\([^)]*\)/g) ?? [],
    };
  });
  const bubble = seen.stops.length === 2
    ? Array.from({ length: 21 }, (_, at) => between(parse(seen.stops[0]), parse(seen.stops[1]), at / 20))
    : [parse(seen.solid)];
  const buttons = bubble.map((under) => over(parse(seen.button), under));
  return {
    ...seen,
    label: seen.label,
    words: Math.min(...buttons.map((button) => contrast(over(parse(seen.label), button), button))),
    edge: Math.min(...buttons.map((button, at) => contrast(button, bubble[at]))),
  };
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

      for (const side of ["mine", "theirs"]) {
        test(`Play reads, and reads as a button, in ${side === "mine" ? "this phone's" : "the other one's"} bubble`, async ({ app }) => {
          test.fail(PALETTE_UNDER.has(`${direction} ${appearance} ${side}`), "the light palette itself is under the line here");
          const play = app.locator(`.ft-msg.is-${side} [data-test='play-game']`);
          await expect(play).toBeVisible();
          const seen = await measure(play);
          const where = `label ${seen.label} on ${seen.button} over ${seen.stops.join(" → ") || seen.solid}`;
          expect(seen.words, `words: ${where}`).toBeGreaterThanOrEqual(4.5);
          expect(seen.edge, `button on bubble: ${where}`).toBeGreaterThanOrEqual(3);
        });
      }
    });
  }
}
