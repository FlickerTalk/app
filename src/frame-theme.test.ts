// The theme Ionic reads inside a plugin's frame (`src-tauri/src/frame-theme.js`, served as
// `ionic/theme.js` to every frame, 2026-10-09): the app hands the frame nine colours and whether it
// is dark; Ionic's components read many more (the numbers of a colour, the steps between page and
// text, the shades of each colour, the overlays). This derives them, so every plugin gets them.
import { afterEach, describe, expect, it } from "vitest";
import source from "../src-tauri/src/frame-theme.js?raw";

const EMBER_DARK: Record<string, string> = {
  "--ion-background-color": "#0d0b0a",
  "--ion-text-color": "#f6efe8",
  "--ion-color-medium": "#9b8f86",
  "--ion-item-background": "#161311",
  "--ion-border-color": "rgba(255, 240, 225, 0.07)",
  "--ion-color-primary": "#ffa24c",
  "--ion-color-primary-contrast": "#1b0f06",
  "--ion-color-success": "#2dd55b",
  "--ion-color-danger": "#ff4d5e",
};

const EMBER_LIGHT: Record<string, string> = {
  ...EMBER_DARK,
  "--ion-background-color": "#fbf8f5",
  "--ion-text-color": "#1c1714",
  "--ion-item-background": "#ffffff",
  "--ion-color-primary": "#9e4c10",
  "--ion-color-primary-contrast": "#ffffff",
};

const root = () => document.documentElement;

/** Puts the colours on the root as the frame's own script does (`frame.js`, `paint`). */
function paint(colours: Record<string, string>, dark: boolean) {
  root().removeAttribute("style");
  for (const [name, value] of Object.entries(colours)) root().style.setProperty(name, value);
  if (dark) root().dataset.dark = "1";
  else delete root().dataset.dark;
}

/** What the script derived, as a map of variable to value. */
function derived(): Record<string, string> {
  const style = document.getElementById("ft-ionic-theme");
  const out: Record<string, string> = {};
  for (const [, name, value] of (style?.textContent ?? "").matchAll(/(--[a-z0-9-]+)\s*:\s*([^;}]+)/g)) out[name] = value.trim();
  return out;
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

let stop: (() => void) | undefined;
async function run() {
  // The script runs once, as a module of the frame; its observer is stopped after each test.
  const observer = new Function(`${source}\nreturn observer;`)() as MutationObserver;
  stop = () => observer.disconnect();
  await settle();
}

afterEach(() => {
  stop?.();
  stop = undefined;
  document.getElementById("ft-ionic-theme")?.remove();
  root().removeAttribute("style");
  delete root().dataset.dark;
});

describe("the theme of the frame", () => {
  it("gives Ionic the numbers of the page and the text", async () => {
    paint(EMBER_DARK, true);
    await run();
    expect(derived()).toMatchObject({
      "--ion-background-color-rgb": "13, 11, 10",
      "--ion-text-color-rgb": "246, 239, 232",
    });
  });

  it("steps from the page to the text, under Ionic's new names and its old ones", async () => {
    paint(EMBER_DARK, true);
    await run();
    const theme = derived();
    // 5 % of the text into the page, and 15 % of the page into the text.
    expect(theme["--ion-background-color-step-50"]).toBe("rgb(25, 22, 21)");
    expect(theme["--ion-color-step-50"]).toBe("rgb(25, 22, 21)");
    expect(theme["--ion-text-color-step-150"]).toBe("rgb(211, 205, 199)");
    for (let step = 50; step <= 950; step += 50) {
      for (const name of [`--ion-background-color-step-${step}`, `--ion-text-color-step-${step}`, `--ion-color-step-${step}`]) {
        expect(theme[name], name).toMatch(/^rgb\(\d+, \d+, \d+\)$/);
      }
    }
  });

  it("completes every colour it is handed, as Ionic's colour generator does", async () => {
    paint(EMBER_DARK, true);
    await run();
    const theme = derived();
    expect(theme).toMatchObject({
      "--ion-color-primary-rgb": "255, 162, 76",
      "--ion-color-primary-contrast-rgb": "27, 15, 6",
      // 12 % black and 10 % white, Ionic's own shade and tint.
      "--ion-color-primary-shade": "rgb(224, 143, 67)",
      "--ion-color-primary-tint": "rgb(255, 171, 94)",
      "--ion-color-danger-rgb": "255, 77, 94",
      // A bright colour has dark text on it, a deep one white, as in Ionic's own palettes.
      "--ion-color-danger-contrast": "#000000",
      "--ion-color-danger-contrast-rgb": "0, 0, 0",
      "--ion-color-medium-rgb": "155, 143, 134",
      "--ion-color-success-rgb": "45, 213, 91",
    });
    expect(theme).not.toHaveProperty("--ion-color-primary-contrast");
    // What the app handed over stays the app's: none of the nine is written again.
    for (const name of Object.keys(EMBER_DARK)) expect(theme, name).not.toHaveProperty(name);
  });

  it("makes light the surface of a card and dark the text, each with its contrast", async () => {
    paint(EMBER_DARK, true);
    await run();
    expect(derived()).toMatchObject({
      "--ion-color-light": "rgb(22, 19, 17)",
      "--ion-color-light-rgb": "22, 19, 17",
      "--ion-color-light-contrast": "rgb(246, 239, 232)",
      "--ion-color-light-contrast-rgb": "246, 239, 232",
      "--ion-color-dark": "rgb(246, 239, 232)",
      "--ion-color-dark-contrast": "rgb(13, 11, 10)",
      "--ion-color-dark-rgb": "246, 239, 232",
    });
    expect(derived()["--ion-color-light-shade"]).toMatch(/^rgb\(/);
    expect(derived()["--ion-color-dark-tint"]).toMatch(/^rgb\(/);
  });

  it("dresses bars, cards and rows as the app does", async () => {
    paint(EMBER_DARK, true);
    await run();
    expect(derived()).toMatchObject({
      "--ion-toolbar-background": "rgb(13, 11, 10)",
      "--ion-toolbar-color": "rgb(246, 239, 232)",
      "--ion-card-background": "rgb(22, 19, 17)",
      "--ion-item-border-color": "rgba(255, 240, 225, 0.07)",
    });
  });

  it("lifts the overlays onto the surface in the dark, and dims more behind them, as the app's sheets", async () => {
    paint(EMBER_DARK, true);
    await run();
    expect(derived()).toMatchObject({ "--ion-overlay-background-color": "rgb(22, 19, 17)", "--ion-backdrop-opacity": "0.6" });
  });

  it("leaves the overlays to Ionic in the light", async () => {
    paint(EMBER_LIGHT, false);
    await run();
    expect(derived()).not.toHaveProperty("--ion-overlay-background-color");
    expect(derived()).not.toHaveProperty("--ion-backdrop-opacity");
    expect(derived()["--ion-background-color-rgb"]).toBe("251, 248, 245");
  });

  it("follows the app when it turns light with the plugin open", async () => {
    paint(EMBER_DARK, true);
    await run();
    paint(EMBER_LIGHT, false);
    await settle();
    expect(derived()["--ion-text-color-rgb"]).toBe("28, 23, 20");
    expect(derived()).not.toHaveProperty("--ion-backdrop-opacity");
  });

  it("reads every way a colour may be written", async () => {
    paint({ "--ion-background-color": "#000", "--ion-text-color": "hsl(0, 0%, 100%)", "--ion-color-primary": "rgb(10 20 30 / 50%)" }, false);
    await run();
    expect(derived()).toMatchObject({
      "--ion-background-color-rgb": "0, 0, 0",
      "--ion-text-color-rgb": "255, 255, 255",
      "--ion-color-primary-rgb": "10, 20, 30",
    });
  });

  it("derives nothing it has no colour for: Ionic's own then", async () => {
    paint({ "--ion-text-color": "not a colour" }, false);
    await run();
    const theme = derived();
    expect(theme).not.toHaveProperty("--ion-text-color-rgb");
    expect(theme).not.toHaveProperty("--ion-background-color-step-50");
    expect(JSON.stringify(theme)).not.toMatch(/NaN|undefined/);
  });
});
