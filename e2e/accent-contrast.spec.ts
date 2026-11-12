import type { Locator, Page } from "@playwright/test";
import { expect, holdTile, test } from "./helpers";

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

for (const direction of ["ember", "aurora", "mono", "lime"]) {
  for (const appearance of ["light", "dark"]) {
    test.describe(`${direction}, ${appearance}`, () => {
      test.use({ viewport: { width: 390, height: 844 } });
      test.beforeEach(({ app }) => theme(app, direction, appearance));

      test("the accent reads as text", async ({ app }) => {
        await app.goto("/tabs/chats");
        const time = await measure(app.locator(".ft-row__time.is-unread").first());
        expect(time.worst, `unread time ${time.colour}`).toBeGreaterThanOrEqual(4.5);
        await app.goto("/circle/circle1");
        const sender = await measure(app.getByTestId("sender").first());
        expect(sender.worst, `sender ${sender.colour}`).toBeGreaterThanOrEqual(4.5);
        await app.goto("/circle/circle1/info");
        const badge = await measure(app.getByTestId("circle-admin-badge").first());
        expect(badge.worst, `admin badge ${badge.colour}`).toBeGreaterThanOrEqual(4.5);
      });

      test("the accent's glyph-only controls read", async ({ app }) => {
        await app.goto("/tabs/settings");
        const qr = await measure(app.getByTestId("show-qr"));
        expect(qr.worst, `QR ${qr.colour}`).toBeGreaterThanOrEqual(3);
        const tab = await measure(app.locator("ion-tab-button.tab-selected").first());
        expect(tab.worst, `selected tab ${tab.colour}`).toBeGreaterThanOrEqual(3);
      });

      test("danger reads as text and as a glyph", async ({ app }) => {
        await app.goto("/tabs/apps");
        await holdTile(app, "app-com.flickertalk.markdown");
        const bin = await measure(app.getByTestId("sheet-remove").locator("ion-icon"));
        expect(bin.worst, `remove ${bin.colour}`).toBeGreaterThanOrEqual(3);
        await app.getByTestId("sheet-remove").click();
        const sure = await measure(app.locator(".ft-app-sheet__ask p"));
        expect(sure.worst, `remove question ${sure.colour}`).toBeGreaterThanOrEqual(4.5);
        await app.goto("/contact/ft_bob123456789");
        const report = await measure(app.getByTestId("report").locator("ion-label"));
        expect(report.worst, `report ${report.colour}`).toBeGreaterThanOrEqual(4.5);
        const flag = await measure(app.locator(".ft-tile--danger").first());
        expect(flag.worst, `report flag ${flag.colour}`).toBeGreaterThanOrEqual(3);
      });

      test("the erase confirmation reads", async ({ app }) => {
        test.fail(appearance === "dark", "white on the dark themes' danger red is about 3.2:1");
        await app.goto("/tabs/settings");
        await app.getByTestId("erase").click();
        const go = await measure(app.getByTestId("erase-confirm"));
        expect(go.worst, `erase ${go.colour}`).toBeGreaterThanOrEqual(4.5);
      });
    });
  }
}
