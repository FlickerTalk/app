// The accent where it carries meaning (decided 2026-10-02: the light palettes are retouched to read):
// as text it needs WCAG AA, 4.5:1, against what is behind it; as the only glyph of a control, 3:1.
// Measured on the page in every colour and appearance: what is behind an element is the first
// opaque background up its ancestors, the see-through ones laid over it.
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

type Rgba = [number, number, number, number];

function parse(css: string): Rgba {
  const numbers = css.match(/[\d.]+/g)!.map(Number);
  if (css.startsWith("color(")) return [numbers[0], numbers[1], numbers[2], numbers[3] ?? 1];
  return [numbers[0] / 255, numbers[1] / 255, numbers[2] / 255, numbers[3] ?? 1];
}

const over = ([r, g, b, a]: Rgba, [R, G, B]: Rgba): Rgba => [r * a + R * (1 - a), g * a + G * (1 - a), b * a + B * (1 - a), 1];

function luminance([r, g, b]: Rgba): number {
  const linear = (c: number) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
}

function contrast(one: Rgba, other: Rgba): number {
  const [light, dark] = [luminance(one), luminance(other)].sort((a, b) => b - a);
  return (light + 0.05) / (dark + 0.05);
}

/** The element's colour, and the backgrounds up its ancestors (a gradient gives its stops). */
async function measure(element: Locator) {
  const seen = await element.evaluate((node) => {
    const layers: string[][] = [];
    for (let at: Element | null = node; at; at = at.parentElement ?? ((at.getRootNode() as ShadowRoot).host ?? null)) {
      const style = getComputedStyle(at);
      const stops = style.backgroundImage.match(/(?:rgba?|color)\([^)]*\)/g);
      if (stops && style.backgroundImage.includes("linear-gradient")) layers.push(stops);
      else if (style.backgroundColor !== "rgba(0, 0, 0, 0)") layers.push([style.backgroundColor]);
      const last = layers.at(-1);
      if (last && last.every((one) => !/,\s*0?\.\d+\)|\/\s*0?\.\d+\)/.test(one))) break;
    }
    return { colour: getComputedStyle(node).color, layers };
  });
  // From the opaque one down to the element, each choice of stop: the worst is what counts.
  let behind: Rgba[] = seen.layers.at(-1)!.map(parse);
  for (const layer of seen.layers.slice(0, -1).reverse()) behind = layer.flatMap((one) => behind.map((under) => over(parse(one), under)));
  return { ...seen, worst: Math.min(...behind.map((under) => contrast(over(parse(seen.colour), under), under))) };
}

async function theme(app: Page, direction: string, appearance: string) {
  await app.addInitScript(
    ([chosenDirection, chosenAppearance]) => {
      localStorage.setItem("ft-direction", chosenDirection);
      localStorage.setItem("ft-appearance", chosenAppearance);
    },
    [direction, appearance],
  );
}

for (const direction of ["ember", "aurora", "mono"]) {
  for (const appearance of ["light", "dark"]) {
    test.describe(`${direction}, ${appearance}`, () => {
      test.use({ viewport: { width: 390, height: 844 } });
      test.beforeEach(({ app }) => theme(app, direction, appearance));

      test("the accent reads as text", async ({ app }) => {
        // The list: an unread circle's time.
        await app.goto("/tabs/chats");
        const time = await measure(app.locator(".ft-row__time.is-unread").first());
        expect(time.worst, `unread time ${time.colour}`).toBeGreaterThanOrEqual(4.5);
        // A circle: who said it, over the other one's bubble.
        await app.goto("/circle/circle1");
        const sender = await measure(app.getByTestId("sender").first());
        expect(sender.worst, `sender ${sender.colour}`).toBeGreaterThanOrEqual(4.5);
        // Its settings: «Admin» on its own tint.
        await app.goto("/circle/circle1/info");
        const badge = await measure(app.getByTestId("circle-admin-badge").first());
        expect(badge.worst, `admin badge ${badge.colour}`).toBeGreaterThanOrEqual(4.5);
      });

      test("the accent's glyph-only controls read", async ({ app }) => {
        // Settings: the QR button, white on the accent.
        await app.goto("/tabs/settings");
        const qr = await measure(app.getByTestId("show-qr"));
        expect(qr.worst, `QR ${qr.colour}`).toBeGreaterThanOrEqual(3);
        // The tab bar: the selected tab is its icon in the accent.
        const tab = await measure(app.locator("ion-tab-button.tab-selected").first());
        expect(tab.worst, `selected tab ${tab.colour}`).toBeGreaterThanOrEqual(3);
      });
    });
  }
}
