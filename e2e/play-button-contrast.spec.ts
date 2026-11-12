import type { Locator } from "@playwright/test";
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const INVITATION = "🎮 Shall we play Tic-tac-toe? https://flickertalk.com/games/tictactoe";

type Rgba = [number, number, number, number];

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

for (const direction of ["ember", "aurora", "mono", "lime"]) {
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
          const play = app.locator(`.ft-msg.is-${side} [data-test='play-game']`);
          await expect(play).toBeVisible();
          const seen = await measure(play);
          const where = `label ${seen.label} on ${seen.button} over ${seen.stops.join(" → ") || seen.solid}`;
          expect(seen.words, `words: ${where}`).toBeGreaterThanOrEqual(4.5);
          expect(seen.edge, `button on bubble: ${where}`).toBeGreaterThanOrEqual(3);
        });
      }

      for (const part of ["text", "time"] as const) {
        test(`this phone's ${part === "text" ? "message" : "time"} reads over all its bubble`, async ({ app }) => {
          const bubble = app.locator(".ft-msg.is-mine .ft-bubble").first();
          await expect(bubble).toBeVisible();
          const seen = await bubble.evaluate((node, which) => {
            const element = which === "text" ? node.querySelector(".ft-bubble__text")! : node.querySelector(".ft-bubble__meta")!;
            return {
              colour: getComputedStyle(element).color,
              opacity: Number(getComputedStyle(element).opacity),
              stops: getComputedStyle(node).backgroundImage.match(/(?:rgba?|color)\([^)]*\)/g)!,
            };
          }, part);
          const [from, to] = seen.stops.map(parse);
          const ratios = Array.from({ length: 21 }, (_, at) => {
            const under = between(from, to, at / 20);
            const [r, g, b, a] = parse(seen.colour);
            return contrast(over([r, g, b, a * seen.opacity], under), under);
          });
          expect(Math.min(...ratios), `${seen.colour} at ${seen.opacity} over ${seen.stops.join(" → ")}`).toBeGreaterThanOrEqual(4.5);
        });
      }

      test("the other one's time reads", async ({ app }) => {
        const bubble = app.locator(".ft-msg.is-theirs .ft-bubble:not(.is-media)").first();
        await expect(bubble).toBeVisible();
        const seen = await bubble.evaluate((node) => {
          const meta = node.querySelector(".ft-bubble__meta")!;
          return { colour: getComputedStyle(meta).color, opacity: Number(getComputedStyle(meta).opacity), under: getComputedStyle(node).backgroundColor };
        });
        const under = parse(seen.under);
        const [r, g, b, a] = parse(seen.colour);
        expect(contrast(over([r, g, b, a * seen.opacity], under), under), `${seen.colour} at ${seen.opacity} over ${seen.under}`).toBeGreaterThanOrEqual(4.5);
      });
    });
  }
}
