import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import variablesCss from "./theme/variables.css?raw";
import { applyAppearance, applyDirection, storedAppearance, storedDirection } from "./theme";

function systemPrefersDark(dark: boolean) {
  vi.spyOn(window, "matchMedia").mockReturnValue({
    matches: dark,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  } as unknown as MediaQueryList);
}

describe("theme", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => vi.restoreAllMocks());

  // Black and white is the default (Ioan, 2026-09-22); the colours are a choice.
  it("uses the black and white colors until the user picks others", () => {
    expect(storedDirection()).toBe("mono");
  });

  it("remembers the colors the user picked", () => {
    applyDirection("aurora");
    expect(document.documentElement.dataset.direction).toBe("aurora");
    expect(storedDirection()).toBe("aurora");
  });

  it("ignores unknown stored colors", () => {
    localStorage.setItem("ft-direction", "neon");
    expect(storedDirection()).toBe("mono");
  });

  it("is dark by default", () => {
    expect(storedAppearance()).toBe("dark");
  });

  it("applies and remembers light and dark appearance", () => {
    applyAppearance("light");
    expect(document.documentElement.classList.contains("ft-dark")).toBe(false);
    applyAppearance("dark");
    expect(document.documentElement.classList.contains("ft-dark")).toBe(true);
    expect(storedAppearance()).toBe("dark");
  });

  it("follows the system when asked to", () => {
    systemPrefersDark(false);
    applyAppearance("system");
    expect(document.documentElement.classList.contains("ft-dark")).toBe(false);
    expect(storedAppearance()).toBe("system");
  });
});

// 2026-10-03: Ionic draws some parts (an unselected segment, an item's icon, a toggle's track) with
// `rgba(var(--ion-text-color-rgb, 0, 0, 0), …)` and `--ion-background-color-rgb`. Without them a
// dark app showed those parts black on black (the apps sheet). Every theme gives Ionic the same
// colour as its text and background, as numbers.
describe("the theme handed to Ionic", () => {
  const css = variablesCss;
  const blocks = [...css.matchAll(/(html(?:\.ft-dark)?\[data-direction="\w+"\])\s*\{([^}]*)\}/g)].map(([, selector, body]) => ({ selector, body }));
  const token = (body: string, name: string) => body.match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1].trim();
  const rgb = (hex: string) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(", ");

  it("gives every theme its text and background colours as numbers too", () => {
    const themes = blocks.filter((one) => token(one.body, "ft-text"));
    expect(themes).toHaveLength(6);
    for (const { selector, body } of themes) {
      expect(token(body, "ft-text-rgb"), selector).toBe(rgb(token(body, "ft-text")!));
      expect(token(body, "ft-bg-rgb"), selector).toBe(rgb(token(body, "ft-bg")!));
    }
  });

  it("maps them onto Ionic's", () => {
    const ionic = css.match(/html\[data-direction\] body\s*\{([^}]*)\}/)![1];
    expect(token(ionic, "ion-text-color-rgb")).toBe("var(--ft-text-rgb)");
    expect(token(ionic, "ion-background-color-rgb")).toBe("var(--ft-bg-rgb)");
  });

  // As Ionic's own dark palette does: in the dark, a modal (the apps sheet) is lifted onto the
  // surface colour, or on a black page it would have no edge at all.
  it("lifts a modal onto the surface colour in the dark", () => {
    const dark = css.match(/html\.ft-dark\[data-direction\] ion-modal\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(token(dark, "ion-background-color")).toBe("var(--ft-surface)");
    expect(token(dark, "ion-toolbar-background")).toBe("var(--ft-surface)");
  });
});
