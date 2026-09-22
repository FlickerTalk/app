import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
