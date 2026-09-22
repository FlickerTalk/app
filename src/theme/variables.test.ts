import { describe, expect, it } from "vitest";
import css from "./variables.css?raw";

function blocks(selectorStart: string): string[] {
  return css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("}").filter((block) => block.trim().startsWith(selectorStart));
}

describe("theme variables", () => {
  // Ionic's Material components (toggle track, focus rings) read the accent as "r, g, b".
  it("gives every color, light and dark, its accent as RGB for Ionic", () => {
    const palettes = [...blocks('html[data-direction="'), ...blocks('html.ft-dark[data-direction="')];
    expect(palettes).toHaveLength(6);
    for (const palette of palettes) {
      expect(palette).toMatch(/--ft-accent-rgb:\s*\d+,\s*\d+,\s*\d+;/);
    }
  });

  // The default look is black and white: every colour of the mono palette is a grey.
  it("keeps the mono palette black and white", () => {
    const palettes = [...blocks('html[data-direction="mono"'), ...blocks('html.ft-dark[data-direction="mono"')];
    expect(palettes).toHaveLength(2);
    for (const palette of palettes) {
      for (const [colour, value] of palette.matchAll(/#([0-9a-f]{6})\b/gi)) {
        const [r, g, b] = [0, 2, 4].map((i) => value.slice(i, i + 2));
        expect(`${colour} ${r === g && g === b}`).toBe(`${colour} true`);
      }
      for (const [colour, channels] of palette.matchAll(/rgba?\(([^)]+)\)/gi)) {
        const [r, g, b] = channels.split(/[\s,/]+/).filter(Boolean).slice(0, 3);
        expect(`${colour} ${r === g && g === b}`).toBe(`${colour} true`);
      }
      const [, accent] = palette.match(/--ft-accent-rgb:\s*(\d+),\s*(\d+),\s*(\d+);/) ?? [];
      expect(palette).toMatch(/--ft-accent-rgb:\s*(\d+),\s*\1,\s*\1;/);
      expect(accent === undefined || true).toBe(true);
    }
  });

  it("hands that RGB accent to Ionic", () => {
    expect(css).toContain("--ion-color-primary-rgb: var(--ft-accent-rgb);");
  });
});
