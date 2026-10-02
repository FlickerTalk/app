import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import variablesCss from "./theme/variables.css?raw";
import { installTauri } from "./__tests__/tauri";
import {
  applyAppearance,
  applyDirection,
  darkScreen,
  initTheme,
  PLUGIN_COLOURS,
  pluginTheme,
  storedAppearance,
  storedDirection,
} from "./theme";

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

  // The first Mono, black with an electric lime, came back as a fourth choice (Ioan, 2026-10-02).
  it("applies and remembers the lime colors", () => {
    applyDirection("lime");
    expect(document.documentElement.dataset.direction).toBe("lime");
    expect(storedDirection()).toBe("lime");
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

// 2026-10-02: the system bars' icons follow the app's appearance, not the system's (on Android
// they were white on the light app). The page tells the phone whether it is dark when it applies
// its appearance and each time that changes; a dark screen (a video call, the camera) asks for
// light icons whatever the appearance.
describe("the system bars", () => {
  let told: boolean[];
  let systemChanged: (() => void) | undefined;

  beforeEach(() => {
    localStorage.clear();
    told = [];
    installTauri((command, args) => {
      if (command === "core_system_bars") told.push(args?.dark as boolean);
      return undefined;
    });
  });
  afterEach(() => {
    darkScreen("test", false);
    installTauri();
  });

  const lastTold = () => told.at(-1);
  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

  it("are told the app is dark when it starts dark, the default", async () => {
    initTheme();
    await settle();
    expect(lastTold()).toBe(true);
  });

  it("are told each change of appearance", async () => {
    applyAppearance("light");
    await settle();
    expect(lastTold()).toBe(false);
    applyAppearance("dark");
    await settle();
    expect(lastTold()).toBe(true);
  });

  it("follow the system when the appearance does", async () => {
    let dark = false;
    vi.spyOn(window, "matchMedia").mockReturnValue({
      get matches() {
        return dark;
      },
      addEventListener: (_: string, listener: () => void) => (systemChanged = listener),
      removeEventListener: () => undefined,
    } as unknown as MediaQueryList);
    applyAppearance("system");
    await settle();
    expect(lastTold()).toBe(false);
    dark = true;
    systemChanged?.();
    await settle();
    expect(lastTold()).toBe(true);
  });

  it("get light icons over a dark screen on the light app, and the app's again after it", async () => {
    applyAppearance("light");
    darkScreen("test", true);
    await settle();
    expect(lastTold()).toBe(true);
    expect(document.documentElement.classList.contains("ft-dark-screen")).toBe(true);
    darkScreen("test", false);
    await settle();
    expect(lastTold()).toBe(false);
    expect(document.documentElement.classList.contains("ft-dark-screen")).toBe(false);
  });

  it("are not bothered when nothing changes", async () => {
    applyAppearance("dark");
    await settle();
    const before = told.length;
    darkScreen("test", true);
    darkScreen("test", true);
    applyAppearance("dark");
    await settle();
    expect(told.length).toBe(before);
  });

  it("never let a missing bridge stop the theme (a plain browser)", async () => {
    installTauri(() => {
      throw new Error("no bridge");
    });
    expect(() => applyAppearance("light")).not.toThrow();
    await settle();
    expect(document.documentElement.classList.contains("ft-dark")).toBe(false);
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
    expect(themes).toHaveLength(8);
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

// Decided 2026-10-02 (the owner): the light palettes are retouched, as little as possible, to read
// at WCAG AA (4.5:1 for normal text). Checked on the palette itself, every theme: the text of this
// phone's bubble over every point of its gradient, and the accent used as text on the page, the
// surfaces and its own tint (a badge). In the light themes also the "Play" label in this phone's
// bubble, over the button's 14 % tint of that text, anywhere on the gradient.
describe("the palettes' contrast", () => {
  const blocks = [...variablesCss.matchAll(/html(\.ft-dark)?\[data-direction="(\w+)"\]\s*\{([^}]*)\}/g)]
    .filter(([, , , body]) => body.includes("--ft-accent:"))
    .map(([, dark, direction, body]) => ({ name: `${direction} ${dark ? "dark" : "light"}`, body }));
  const token = (body: string, name: string) => body.match(new RegExp(`--${name}:\\s*([^;]+);`))![1].trim();
  type Rgb = [number, number, number];
  const parse = (hex: string): Rgb => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16) / 255) as Rgb;
  const over = (top: Rgb, alpha: number, under: Rgb): Rgb => top.map((one, at) => one * alpha + under[at] * (1 - alpha)) as Rgb;
  const luminance = (colour: Rgb) => {
    const [r, g, b] = colour.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const contrast = (one: Rgb, other: Rgb) => {
    const [light, dark] = [luminance(one), luminance(other)].sort((a, b) => b - a);
    return (light + 0.05) / (dark + 0.05);
  };
  /** The bubble's gradient, sampled from one stop to the other. */
  const gradient = (body: string) => {
    const [from, to] = [parse(token(body, "ft-accent")), parse(token(body, "ft-accent-2"))];
    return Array.from({ length: 21 }, (_, at) => over(to, at / 20, from));
  };

  it("finds the six themes", () => {
    expect(blocks.map((one) => one.name).sort()).toEqual(["aurora dark", "aurora light", "ember dark", "ember light", "mono dark", "mono light"]);
  });

  it("keeps the accent's numbers the accent", () => {
    for (const { name, body } of blocks) {
      const hex = token(body, "ft-accent");
      expect(token(body, "ft-accent-rgb"), name).toBe([1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(", "));
    }
  });

  it.each(["ember light", "ember dark", "aurora light", "aurora dark", "mono light", "mono dark"])("%s: this phone's bubble reads", (name) => {
    const { body } = blocks.find((one) => one.name === name)!;
    const text = parse(token(body, "ft-on-accent"));
    const worst = Math.min(...gradient(body).map((under) => contrast(text, under)));
    expect(worst).toBeGreaterThanOrEqual(4.5);
  });

  it.each(["ember light", "ember dark", "aurora light", "aurora dark", "mono light", "mono dark"])("%s: the accent reads as text", (name) => {
    const { body } = blocks.find((one) => one.name === name)!;
    const accent = parse(token(body, "ft-accent"));
    const page = parse(token(body, "ft-bg"));
    for (const under of [page, parse(token(body, "ft-surface")), parse(token(body, "ft-surface-2")), over(accent, 0.16, page)]) {
      expect(contrast(accent, under)).toBeGreaterThanOrEqual(4.5);
    }
  });

  it.each(["ember light", "aurora light"])("%s: Play reads in this phone's bubble, anywhere on it", (name) => {
    const { body } = blocks.find((one) => one.name === name)!;
    const text = parse(token(body, "ft-on-accent"));
    const worst = Math.min(...gradient(body).map((under) => contrast(text, over(text, 0.14, under))));
    expect(worst).toBeGreaterThanOrEqual(4.5);
  });
});

// Danger red (decided 2026-10-02): as text it reads at 4.5:1 on the light themes' page and surfaces;
// Ionic's color="danger" reads the companions, which match the colour in each appearance.
describe("the danger colour", () => {
  const body = (selector: string) => variablesCss.slice(variablesCss.indexOf(`${selector} {`)).split("}")[0];
  const value = (block: string, name: string) => block.match(new RegExp(`--${name}:\\s*([^;]+);`))?.[1].trim();
  const numbers = (hex: string) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16)).join(", ");

  it.each(["html[data-direction] body", "html.ft-dark[data-direction] body"])("%s: the companions match", (selector) => {
    const block = body(selector);
    const danger = value(block, "ion-color-danger")!;
    expect(value(block, "ion-color-danger-rgb")).toBe(numbers(danger));
    for (const name of ["contrast", "shade", "tint"]) expect(value(block, `ion-color-danger-${name}`), name).toMatch(/^#[0-9a-f]{6}$/);
  });

  it("reads as text on every light theme's page and surfaces", () => {
    const danger = value(body("html[data-direction] body"), "ion-color-danger")!;
    const lights = [...variablesCss.matchAll(/html\[data-direction="\w+"\]\s*\{([^}]*)\}/g)].map(([, block]) => block);
    const parse = (hex: string) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16) / 255);
    const luminance = (hex: string) => {
      const [r, g, b] = parse(hex).map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const contrast = (one: string, other: string) => {
      const [light, dark] = [luminance(one), luminance(other)].sort((a, b) => b - a);
      return (light + 0.05) / (dark + 0.05);
    };
    expect(lights).toHaveLength(3);
    for (const block of lights) {
      for (const under of ["ft-bg", "ft-surface", "ft-surface-2"]) expect(contrast(danger, value(block, under)!), under).toBeGreaterThanOrEqual(4.5);
    }
  });
});
