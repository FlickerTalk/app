import { beforeEach, describe, expect, it } from "vitest";
import {
  isOnboarded,
  isTourDue,
  setCallRouting,
  setOnboarded,
  setTourDue,
  setTourSeen,
  storedCallRouting,
  syncCallRouting,
} from "./preferences";
import { installTauri } from "./__tests__/tauri";

describe("preferences", () => {
  beforeEach(() => localStorage.clear());

  it("relays calls only when a direct connection fails", () => {
    expect(storedCallRouting()).toBe("auto");
  });

  it("remembers how the user wants calls routed", () => {
    setCallRouting("always");
    expect(storedCallRouting()).toBe("always");
    setCallRouting("direct");
    expect(storedCallRouting()).toBe("direct");
  });

  // 2026-09-28: a call answered from CallKit has no WebView to ask, so the core keeps a copy.
  it("tells the core how calls are routed", async () => {
    const sent: unknown[] = [];
    installTauri((command, args) => void sent.push([command, args]));
    setCallRouting("always");
    await syncCallRouting();
    expect(sent).toEqual([
      ["core_set_call_routing", { routing: "always" }],
      ["core_set_call_routing", { routing: "always" }],
    ]);
  });

  it("knows whether the welcome screen was already seen", () => {
    expect(isOnboarded()).toBe(false);
    setOnboarded();
    expect(isOnboarded()).toBe(true);
  });

  // 2026-10-28: the quick tour shows once, to a new identity. Start makes it due; an install that
  // was set up before it existed has nothing stored and never sees it on its own.
  it("owes the quick tour only to an identity made with Start, until it is seen", () => {
    setOnboarded();
    expect(isTourDue()).toBe(false);
    setTourDue();
    expect(isTourDue()).toBe(true);
    setTourSeen();
    expect(isTourDue()).toBe(false);
    expect(localStorage.getItem("ft-tour")).toBe("seen");
  });

  it("ignores an unknown stored value", () => {
    localStorage.setItem("ft-call-routing", "whatever");
    expect(storedCallRouting()).toBe("auto");
  });
});
