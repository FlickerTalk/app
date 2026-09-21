import { describe, expect, it } from "vitest";
import { onboardingGuard, routes } from "./router";
import { setOnboarded } from "./preferences";

describe("routes", () => {
  it("redirects the root to the chats tab", () => {
    expect(routes.find((route) => route.path === "/")?.redirect).toBe("/tabs/chats");
  });

  it("exposes chats, calls and settings as tabs", () => {
    const tabs = routes.find((route) => route.path === "/tabs/");
    expect(tabs?.children?.map((child) => child.path)).toEqual(
      expect.arrayContaining(["chats", "calls", "settings"]),
    );
  });

  it("has a screen for the welcome, adding contacts, a contact and a call", () => {
    const paths = routes.map((route) => route.path);
    expect(paths).toEqual(
      expect.arrayContaining(["/welcome", "/add-contact", "/contact/:id", "/call/:id"]),
    );
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
});
