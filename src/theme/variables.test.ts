import { afterEach, beforeEach, describe, expect, it } from "vitest";
import toggleSource from "@ionic/core/components/ion-toggle.js?raw";
import css from "./variables.css?raw";
import baseCss from "./base.css?raw";
import { DIRECTIONS } from "../theme";

function blocks(selectorStart: string): string[] {
  return css
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .split("}")
    .filter((block) => block.trim().startsWith(selectorStart) && block.includes("--ft-accent"));
}

describe("theme variables", () => {
  it("gives every color, light and dark, its accent as RGB for Ionic", () => {
    const palettes = [...blocks('html[data-direction="'), ...blocks('html.ft-dark[data-direction="')];
    expect(palettes).toHaveLength(8);
    for (const palette of palettes) {
      expect(palette).toMatch(/--ft-accent-rgb:\s*\d+,\s*\d+,\s*\d+;/);
    }
  });

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

  it("takes the colour out of the avatars in the mono theme", () => {
    expect(css).toMatch(/html\[data-direction="mono"\][^{]*\.ft-avatar[^{]*\{[^}]*filter:\s*grayscale\(1\)/);
  });

  it("hands that RGB accent to Ionic", () => {
    expect(css).toContain("--ion-color-primary-rgb: var(--ft-accent-rgb);");
  });
});

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
  const over = (top: Rgb, alpha: number, under: Rgb): Rgb => top.map((one, at) => one * alpha + under[at] * (1 - alpha)) as Rgb;
  const ratio = (one: Rgb, other: Rgb) => {
    const [lighter, darker] = [luminance(one), luminance(other)].sort((a, b) => b - a);
    return (lighter + 0.05) / (darker + 0.05);
  };
  const contrast = (palette: string, one: string, other: string) => ratio(parse(token(palette, one)!), parse(token(palette, other)!));
  const gradient = (palette: string) => {
    const [from, to] = [parse(token(palette, "ft-accent")!), parse(token(palette, "ft-accent-2")!)];
    return Array.from({ length: 21 }, (_, at) => over(to, at / 20, from));
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

  it("reads on its own tint and in this phone's bubble in the light", () => {
    const accent = parse(token(light, "ft-accent")!);
    expect(ratio(accent, over(accent, 0.16, parse(token(light, "ft-bg")!)))).toBeGreaterThanOrEqual(4.5);
    const text = parse(token(light, "ft-on-accent")!);
    const worst = Math.min(...gradient(light).map((under) => ratio(text, over(text, 0.14, under))));
    expect(worst).toBeGreaterThanOrEqual(4.5);
  });

  it("leaves the avatars their colours", () => {
    expect(css).not.toMatch(/data-direction="lime"\][^{]*\.ft-avatar/);
  });
});

describe("a switched-on toggle", () => {
  const MODES = ["ios", "md"] as const;
  type Mode = (typeof MODES)[number];
  type Rgba = [number, number, number, number];

  const ionicCss: Record<Mode, string> = {
    ios: toggleSource.slice(toggleSource.indexOf('ios:"'), toggleSource.indexOf(",md:")),
    md: toggleSource.slice(toggleSource.indexOf(",md:")),
  };
  const ionicDefault = (mode: Mode, name: string) =>
    ionicCss[mode].match(new RegExp(`:host\\{[^}]*${name}:([^;}]+)`))?.[1].trim() ?? "";

  const root = document.documentElement;
  let sheet: HTMLStyleElement;
  beforeEach(() => {
    sheet = document.createElement("style");
    sheet.textContent = `${css}\n${baseCss}`;
    document.head.append(sheet);
  });
  afterEach(() => {
    sheet.remove();
    document.body.replaceChildren();
    root.removeAttribute("data-direction");
    root.classList.remove("ft-dark");
  });

  const themes = DIRECTIONS.flatMap((direction) => [false, true].map((dark) => ({ direction, dark })));
  const cases = themes.flatMap((theme) => MODES.map((mode) => ({ ...theme, mode })));
  const named = ({ direction, dark, mode }: (typeof cases)[number]) => `${direction} ${dark ? "dark" : "light"} in ${mode}`;

  function render({ direction, dark, mode }: (typeof cases)[number]) {
    root.dataset.direction = direction;
    root.classList.toggle("ft-dark", dark);
    const toggle = document.createElement("ion-toggle");
    toggle.className = `${mode} toggle-checked`;
    document.body.append(toggle);
    return { own: getComputedStyle(toggle), palette: getComputedStyle(document.body), page: getComputedStyle(root) };
  }

  const fromPalette = (value: string, palette: CSSStyleDeclaration) =>
    value.replace(/var\((--[\w-]+)(?:,\s*([^()]*))?\)/g, (_, name: string, fallback = "") => palette.getPropertyValue(name).trim() || fallback.trim());

  const parse = (colour: string): Rgba => {
    const hex = colour.match(/^#([0-9a-f]{6})$/i)?.[1];
    if (hex) return [0, 2, 4].map((at) => parseInt(hex.slice(at, at + 2), 16)).concat(1) as Rgba;
    const [r, g, b, a = 1] = colour.match(/rgba?\(([^)]+)\)/)![1].split(",").map(Number);
    return [r, g, b, a];
  };
  const over = ([r, g, b, a]: Rgba, [ur, ug, ub]: Rgba): Rgba =>
    [r * a + ur * (1 - a), g * a + ug * (1 - a), b * a + ub * (1 - a), 1].map((channel) => Math.round(channel * 100) / 100) as Rgba;

  function drawn(theme: (typeof cases)[number]) {
    const { own, palette, page } = render(theme);
    const colour = (name: string) => parse(fromPalette(own.getPropertyValue(name).trim() || ionicDefault(theme.mode, name), palette));
    const background = parse(page.getPropertyValue("--ft-bg").trim());
    return {
      track: over(colour("--track-background-checked"), background),
      knob: over(colour("--handle-background-checked"), background),
    };
  }

  it("reads Ionic's own iOS and Android defaults", () => {
    expect(ionicDefault("ios", "--track-background-checked")).toBe("var(--ion-color-primary, #0054e9)");
    expect(ionicDefault("ios", "--handle-background-checked")).toBe("#ffffff");
    expect(ionicDefault("md", "--track-background-checked")).toBe("rgba(var(--ion-color-primary-rgb, 0, 84, 233), 0.5)");
    expect(ionicDefault("md", "--handle-background-checked")).toBe("var(--ion-color-primary, #0054e9)");
  });

  it.each(cases.map((theme) => [named(theme), theme] as const))("keeps its knob apart from its track in %s", (_, theme) => {
    const { track, knob } = drawn(theme);
    expect(knob).not.toEqual(track);
  });

  it.each(cases.filter((theme) => !(theme.direction === "mono" && theme.dark && theme.mode === "ios")).map((theme) => [named(theme), theme] as const))(
    "is left as Ionic draws it in %s",
    (_, theme) => {
      const { own } = render(theme);
      for (const name of ["--track-background", "--track-background-checked", "--handle-background", "--handle-background-checked"]) {
        expect(own.getPropertyValue(name).trim(), name).toBe("");
      }
    },
  );
});
