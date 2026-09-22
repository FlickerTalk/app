import { beforeEach, describe, expect, it } from "vitest";
import { isOnboarded, setCallRouting, setOnboarded, storedCallRouting } from "./preferences";

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
