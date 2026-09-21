import { beforeEach, describe, expect, it } from "vitest";
import { setCallRouting, setMailbox, storedCallRouting, storedMailbox } from "./preferences";

describe("preferences", () => {
  beforeEach(() => localStorage.clear());

  it("keeps the offline mailbox on until the user turns it off", () => {
    expect(storedMailbox()).toBe(true);
    setMailbox(false);
    expect(storedMailbox()).toBe(false);
  });

  it("relays calls only when a direct connection fails", () => {
    expect(storedCallRouting()).toBe("auto");
  });

  it("remembers how the user wants calls routed", () => {
    setCallRouting("always");
    expect(storedCallRouting()).toBe("always");
    setCallRouting("direct");
    expect(storedCallRouting()).toBe("direct");
  });

  it("ignores an unknown stored value", () => {
    localStorage.setItem("ft-call-routing", "whatever");
    expect(storedCallRouting()).toBe("auto");
  });
});
