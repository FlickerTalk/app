import { beforeEach, describe, expect, it } from "vitest";
import { isOnboarded, setCallRouting, setOnboarded, storedCallRouting, syncCallRouting } from "./preferences";
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

  it("ignores an unknown stored value", () => {
    localStorage.setItem("ft-call-routing", "whatever");
    expect(storedCallRouting()).toBe("auto");
  });
});
