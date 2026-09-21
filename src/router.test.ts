import { describe, expect, it } from "vitest";
import { routes } from "./router";

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

  it("opens a conversation full screen, outside the tabs", () => {
    expect(routes.some((route) => route.path === "/chat/:id")).toBe(true);
  });
});
