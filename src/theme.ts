// Visual preferences chosen in Settings. They stay on this device (localStorage), never on a server.
import { invoke } from "@tauri-apps/api/core";

// Black and white first: it is the default (Ioan, 2026-09-22). Lime, the first Mono, is back as
// a choice (Ioan, 2026-10-02).
export const DIRECTIONS = ["mono", "ember", "aurora", "lime"] as const;
export type Direction = (typeof DIRECTIONS)[number];

export const APPEARANCES = ["system", "dark", "light"] as const;
export type Appearance = (typeof APPEARANCES)[number];

const DIRECTION_KEY = "ft-direction";
const APPEARANCE_KEY = "ft-appearance";

const root = () => document.documentElement;

function stored<T extends string>(key: string, allowed: readonly T[], fallback: T): T {
  const value = localStorage.getItem(key);
  return allowed.includes(value as T) ? (value as T) : fallback;
}

export function storedDirection(): Direction {
  return stored(DIRECTION_KEY, DIRECTIONS, "mono");
}

// Dark first (Plan §83–84).
export function storedAppearance(): Appearance {
  return stored(APPEARANCE_KEY, APPEARANCES, "dark");
}

export function applyDirection(direction: Direction) {
  root().dataset.direction = direction;
  localStorage.setItem(DIRECTION_KEY, direction);
}

let stopFollowingSystem: (() => void) | undefined;

/** Screens dark whatever the appearance (a video call, the camera), by who asked. */
const darkScreens = new Set<string>();
/** What the system bars were last told: dark or not. */
let toldDark: boolean | undefined;

/**
 * The system bars' icons follow what is on the screen (2026-10-02): light over the dark app or a
 * dark screen, dark over the light app. Only Android acts on it (the bridge's `setSystemBars`);
 * a plain browser has no bridge and nothing breaks.
 */
function tellSystemBars() {
  const dark = root().classList.contains("ft-dark") || darkScreens.size > 0;
  if (dark === toldDark) return;
  toldDark = dark;
  void invoke("core_system_bars", { dark }).catch(() => undefined);
}

function setDark(dark: boolean) {
  root().classList.toggle("ft-dark", dark);
  tellSystemBars();
}

/** A screen that is dark whatever the appearance (`key`: who shows it) comes or goes. */
export function darkScreen(key: string, on: boolean) {
  if (on) darkScreens.add(key);
  else darkScreens.delete(key);
  root().classList.toggle("ft-dark-screen", darkScreens.size > 0);
  tellSystemBars();
}

export function applyAppearance(appearance: Appearance) {
  stopFollowingSystem?.();
  stopFollowingSystem = undefined;
  localStorage.setItem(APPEARANCE_KEY, appearance);

  if (appearance !== "system") {
    setDark(appearance === "dark");
    return;
  }

  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const follow = () => setDark(query.matches);
  follow();
  query.addEventListener("change", follow);
  stopFollowingSystem = () => query.removeEventListener("change", follow);
}

export function initTheme() {
  applyDirection(storedDirection());
  // A new page tells the bars at once, whatever an earlier page told them.
  toldDark = undefined;
  applyAppearance(storedAppearance());
}

/**
 * The colours a plugin is handed (2026-10-02): its frame is isolated, so the app's CSS variables
 * never reach it. Ionic's names, so a plugin's `var(--ion-text-color, …)` follows the app.
 */
export const PLUGIN_COLOURS = [
  "--ion-background-color",
  "--ion-text-color",
  "--ion-color-medium",
  "--ion-item-background",
  "--ion-border-color",
  "--ion-color-primary",
  "--ion-color-primary-contrast",
  "--ion-color-success",
  "--ion-color-danger",
];

/** Where the app keeps a colour a plugin gets under another name: a row of the app is see-through, a plugin's is the surface of a card. */
const PLUGIN_SOURCES: Record<string, string> = { "--ion-item-background": "--ion-card-background" };

/**
 * Whether the app is dark, which way its text runs (2026-10-09, for Ionic inside the frame) and the
 * colours it shows right now, as the plugin gets them.
 */
export function pluginTheme(): { dark: boolean; dir: "rtl" | "ltr"; theme: Record<string, string> } {
  // Ionic's variables are set on the body (variables.css), so that is where they are read.
  const computed = getComputedStyle(document.body);
  const theme: Record<string, string> = {};
  for (const name of PLUGIN_COLOURS) {
    const value = computed.getPropertyValue(PLUGIN_SOURCES[name] ?? name).trim();
    if (value) theme[name] = value;
  }
  return { dark: root().classList.contains("ft-dark"), dir: root().dir === "rtl" ? "rtl" : "ltr", theme };
}
