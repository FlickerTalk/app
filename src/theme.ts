import { invoke } from "@tauri-apps/api/core";

// the first is the default
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

export function storedAppearance(): Appearance {
  return stored(APPEARANCE_KEY, APPEARANCES, "dark");
}

export function applyDirection(direction: Direction) {
  root().dataset.direction = direction;
  localStorage.setItem(DIRECTION_KEY, direction);
}

let stopFollowingSystem: (() => void) | undefined;

const darkScreens = new Set<string>();
let toldDark: boolean | undefined;

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
  void invoke("core_set_appearance", { appearance }).catch(() => undefined);

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
  toldDark = undefined;
  applyAppearance(storedAppearance());
}

// a plugin frame never sees the app's CSS variables, so it gets these under Ionic's names
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

const PLUGIN_SOURCES: Record<string, string> = { "--ion-item-background": "--ion-card-background" };

export function pluginTheme(): { dark: boolean; dir: "rtl" | "ltr"; theme: Record<string, string> } {
  // Ionic's variables live on body
  const computed = getComputedStyle(document.body);
  const theme: Record<string, string> = {};
  for (const name of PLUGIN_COLOURS) {
    const value = computed.getPropertyValue(PLUGIN_SOURCES[name] ?? name).trim();
    if (value) theme[name] = value;
  }
  return { dark: root().classList.contains("ft-dark"), dir: root().dir === "rtl" ? "rtl" : "ltr", theme };
}
