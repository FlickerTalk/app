import { describe, expect, it } from "vitest";
import { onboardingGuard, routes } from "./router";
import { setOnboarded } from "./preferences";

describe("routes", () => {
  it("redirects the root to the chats tab", () => {
    expect(routes.find((route) => route.path === "/")?.redirect).toBe("/tabs/chats");
  });

  it("exposes chats, calls, apps and settings as tabs", () => {
    const tabs = routes.find((route) => route.path === "/tabs/");
    expect(tabs?.children?.map((child) => child.path)).toEqual(expect.arrayContaining(["chats", "calls", "apps", "settings"]));
    expect(tabs?.children?.find((child) => child.path === "apps")?.component).toBeDefined();
  });

  it("sends the old games and plugins addresses to the Apps tab", () => {
    const tabs = routes.find((route) => route.path === "/tabs/");
    for (const path of ["games", "plugins"]) {
      const child = tabs?.children?.find((one) => one.path === path);
      expect(child?.redirect, path).toBe("/tabs/apps");
      expect(child?.component, path).toBeUndefined();
    }
    const plugins = routes.find((route) => route.path === "/plugins");
    expect(plugins?.redirect).toBe("/tabs/apps");
    expect(plugins?.component).toBeUndefined();
  });

  it("has a screen for the welcome, adding contacts, a contact and a call", () => {
    const paths = routes.map((route) => route.path);
    expect(paths).toEqual(
      expect.arrayContaining(["/welcome", "/add-contact", "/contact/:id", "/call/:id", "/move", "/blocked"]),
    );
    expect(paths).not.toContain("/poc");
  });

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
  it("sends the old Plan address to the Premium section of Settings", () => {
    expect(routes.find((route) => route.path === "/plan")?.redirect).toEqual({ path: "/tabs/settings", hash: "#premium" });
    expect(routes.find((route) => route.path === "/plan")?.component).toBeUndefined();
  });
});
