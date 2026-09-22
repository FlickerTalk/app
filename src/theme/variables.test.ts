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

  it("hands that RGB accent to Ionic", () => {
    expect(css).toContain("--ion-color-primary-rgb: var(--ft-accent-rgb);");
  });
});
