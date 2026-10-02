import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import variablesCss from "./theme/variables.css?raw";
import { applyAppearance, applyDirection, PLUGIN_COLOURS, pluginTheme, storedAppearance, storedDirection } from "./theme";

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

// 2026-10-02: Ionic draws some parts (an unselected segment, an item's icon, a toggle's track) with
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

  // Ionic dims the page under a sheet by `--ion-backdrop-opacity` times the sheet's height (0.32 ×
  // 0.5 at half height): on a black page that is no dimming at all. In the dark it dims twice as
  // much, so a sheet reads as lifted off the page (seen on the Samsung, 2026-10-02).
  it("dims the page under a modal enough to be seen in the dark", () => {
    const dark = css.match(/html\.ft-dark\[data-direction\] ion-modal\s*\{([^}]*)\}/)?.[1] ?? "";
    expect(Number(token(dark, "ion-backdrop-opacity"))).toBeGreaterThanOrEqual(0.6);
  });
});

// 2026-10-02 (Ioan): a plugin's frame is isolated, so the app hands it its colours: Ionic's names,
// with the values the app shows right now, and whether it is dark.
describe("the colours a plugin is handed", () => {
  afterEach(() => {
    document.body.removeAttribute("style");
    document.documentElement.classList.remove("ft-dark");
  });

  it("are the nine Ionic colours, as the app computes them, and whether it is dark", () => {
    expect(PLUGIN_COLOURS).toEqual([
      "--ion-background-color",
      "--ion-text-color",
      "--ion-color-medium",
      "--ion-item-background",
      "--ion-border-color",
      "--ion-color-primary",
      "--ion-color-primary-contrast",
      "--ion-color-success",
      "--ion-color-danger",
    ]);
    const body = document.body.style;
    PLUGIN_COLOURS.forEach((name, at) => body.setProperty(name, ` #00000${at}`));
    // A row of the app is see-through; a plugin is handed the surface of a card instead.
    body.setProperty("--ion-item-background", "transparent");
    body.setProperty("--ion-card-background", "#111111");

    const light = pluginTheme();
    expect(light.dark).toBe(false);
    expect(light.theme["--ion-text-color"]).toBe("#000001");
    expect(light.theme["--ion-item-background"]).toBe("#111111");
    expect(Object.keys(light.theme)).toEqual(PLUGIN_COLOURS);

    document.documentElement.classList.add("ft-dark");
    expect(pluginTheme().dark).toBe(true);
  });

  it("leave out a colour the app does not have", () => {
    document.body.style.setProperty("--ion-text-color", "#ffffff");
    expect(pluginTheme().theme).toEqual({ "--ion-text-color": "#ffffff" });
  });

  // Every colour comes from a design token: none is made up for plugins.
  it("are all defined by the app's tokens", () => {
    const block = variablesCss.slice(variablesCss.indexOf("html[data-direction] body {"));
    for (const name of ["--ion-color-medium", "--ion-card-background", "--ion-color-danger", "--ion-border-color"]) {
      expect(block).toMatch(new RegExp(`${name}:`));
    }
    expect(block).toMatch(/--ion-color-medium:\s*var\(--ft-muted\)/);
  });
});
