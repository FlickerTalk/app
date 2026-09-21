// Visual preferences chosen in Settings. They stay on this device (localStorage), never on a server.

export const DIRECTIONS = ["ember", "aurora", "mono"] as const;
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
  return stored(DIRECTION_KEY, DIRECTIONS, "ember");
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

export function applyAppearance(appearance: Appearance) {
  stopFollowingSystem?.();
  stopFollowingSystem = undefined;
  localStorage.setItem(APPEARANCE_KEY, appearance);

  if (appearance !== "system") {
    root().classList.toggle("ft-dark", appearance === "dark");
    return;
  }

  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const follow = () => root().classList.toggle("ft-dark", query.matches);
  follow();
  query.addEventListener("change", follow);
  stopFollowingSystem = () => query.removeEventListener("change", follow);
}

export function initTheme() {
  applyDirection(storedDirection());
  applyAppearance(storedAppearance());
}
