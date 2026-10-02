import { describe, expect, it } from "vitest";
import css from "./variables.css?raw";

/** The palette blocks: the ones that set the `--ft-*` tokens, not every rule with that selector. */
function blocks(selectorStart: string): string[] {
  return css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("}")
    .filter((block) => block.trim().startsWith(selectorStart) && block.includes("--ft-accent"));
}

describe("theme variables", () => {
  // Ionic's Material components (toggle track, focus rings) read the accent as "r, g, b".
  it("gives every color, light and dark, its accent as RGB for Ionic", () => {
    const palettes = [...blocks('html[data-direction="'), ...blocks('html.ft-dark[data-direction="')];
    expect(palettes).toHaveLength(8);
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

  // The black and white theme has no colours: the contact avatars are greys too.
  it("takes the colour out of the avatars in the mono theme", () => {
    expect(css).toMatch(/html\[data-direction="mono"\][^{]*\.ft-avatar[^{]*\{[^}]*filter:\s*grayscale\(1\)/);
  });

  it("hands that RGB accent to Ionic", () => {
    expect(css).toContain("--ion-color-primary-rgb: var(--ft-accent-rgb);");
  });
});

// The first Mono (black with an electric lime) came back as a fourth theme (Ioan, 2026-10-02): the
// dark palette exactly as it was; in the light one the lime is darkened, same hue, to read on white.
describe("the lime palette", () => {
  const [light] = blocks('html[data-direction="lime"');
  const [dark] = blocks('html.ft-dark[data-direction="lime"');
  const token = (palette: string, name: string) => palette.match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1].trim();

  type Rgb = [number, number, number];
  const parse = (hex: string): Rgb => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16) / 255) as Rgb;
  const luminance = (colour: Rgb) => {
    const [r, g, b] = colour.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  /** WCAG contrast ratio between two tokens of a palette. */
  const contrast = (palette: string, one: string, other: string) => {
    const [lighter, darker] = [luminance(parse(token(palette, one)!)), luminance(parse(token(palette, other)!))].sort((a, b) => b - a);
    return (lighter + 0.05) / (darker + 0.05);
  };

  it("exists light and dark", () => {
    expect(light).toBeDefined();
    expect(dark).toBeDefined();
  });

  it("is the original electric lime in the dark", () => {
    expect(token(dark, "ft-bg")).toBe("#000000");
    expect(token(dark, "ft-accent")).toBe("#c6f432");
    expect(token(dark, "ft-accent-rgb")).toBe("198, 244, 50");
  });

  // Nothing left undefined: the same tokens as the other themes.
  it("sets every token the mono palette sets", () => {
    const names = (palette: string) => [...palette.matchAll(/(--ft-[\w-]+):/g)].map(([, name]) => name).sort();
    const [mono] = blocks('html[data-direction="mono"');
    expect(names(light)).toEqual(names(mono));
    expect(names(dark)).toEqual(names(mono));
  });

  it("keeps the accent's numbers the accent", () => {
    for (const palette of [light, dark]) {
      const hex = token(palette, "ft-accent")!;
      expect(token(palette, "ft-accent-rgb")).toBe([1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(", "));
    }
  });

  // WCAG AA for normal text: 4.5:1.
  it.each([
    ["light", () => light],
    ["dark", () => dark],
  ])("reads at WCAG AA in the %s", (_, palette) => {
    expect(contrast(palette(), "ft-on-accent", "ft-accent")).toBeGreaterThanOrEqual(4.5);
    expect(contrast(palette(), "ft-on-accent", "ft-accent-2")).toBeGreaterThanOrEqual(4.5);
    for (const under of ["ft-bg", "ft-surface", "ft-surface-2"]) {
      expect(contrast(palette(), "ft-accent", under), under).toBeGreaterThanOrEqual(4.5);
    }
    expect(contrast(palette(), "ft-text", "ft-bg")).toBeGreaterThanOrEqual(4.5);
  });

  // Lime is a colour theme: its avatars keep their colours.
  it("leaves the avatars their colours", () => {
    expect(css).not.toMatch(/data-direction="lime"\][^{]*\.ft-avatar/);
  });
});
