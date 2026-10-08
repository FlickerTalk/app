import { describe, expect, it } from "vitest";
import { onboardingGuard, routes } from "./router";
import { setOnboarded } from "./preferences";

describe("routes", () => {
  it("redirects the root to the chats tab", () => {
    expect(routes.find((route) => route.path === "/")?.redirect).toBe("/tabs/chats");
  });

  it("exposes chats, calls, games, plugins and settings as tabs", () => {
    const tabs = routes.find((route) => route.path === "/tabs/");
    expect(tabs?.children?.map((child) => child.path)).toEqual(
      expect.arrayContaining(["chats", "calls", "games", "plugins", "settings"]),
    );
  });

  // The plugins keep their own screen too: Settings opens it, and so does a plugin's reminder.
  it("keeps the plugins screen outside the tabs as well", () => {
    expect(routes.some((route) => route.path === "/plugins")).toBe(true);
  });

  it("has a screen for the welcome, adding contacts, a contact and a call", () => {
    const paths = routes.map((route) => route.path);
    expect(paths).toEqual(
      expect.arrayContaining(["/welcome", "/add-contact", "/contact/:id", "/call/:id", "/move", "/blocked"]),
    );
    // PoC 0 is over (§87): its test screen is not part of the app.
    expect(paths).not.toContain("/poc");
  });

  // §60: a new phone may bring the identity of the old one instead of starting fresh.
  it("lets a first run move in from an old phone", () => {
    localStorage.clear();
    expect(onboardingGuard("/move")).toBe(true);
  });

  it("sends a first run to the welcome screen", () => {
    localStorage.clear();
    expect(onboardingGuard("/tabs/chats")).toBe("/welcome");
    expect(onboardingGuard("/welcome")).toBe(true);
    setOnboarded();
    expect(onboardingGuard("/tabs/chats")).toBe(true);
  });

  it("opens a conversation full screen, outside the tabs", () => {
    expect(routes.some((route) => route.path === "/chat/:id")).toBe(true);
  });
  // Ioan, 2026-10-08: the Plan screen is gone; its address leads to the Premium section of Settings.
  it("sends the old Plan address to the Premium section of Settings", () => {
    expect(routes.find((route) => route.path === "/plan")?.redirect).toEqual({ path: "/tabs/settings", hash: "#premium" });
    expect(routes.find((route) => route.path === "/plan")?.component).toBeUndefined();
  });
});
