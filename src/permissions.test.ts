import { describe, expect, it } from "vitest";
import { hostOf, missingPermission, permissionsOf, withPermission } from "./permissions";
import type { PluginView } from "./core";

const BOARD: PluginView = {
  id: "com.flickertalk.board",
  name: "Board",
  version: "1.0.0",
  asks: { network: ["api.example.com"], messages: true, send: "propose", live: true, remind: true, drive: true, location: true, storage: "large" },
  granted: { network: [], messages: false, send: "nothing", live: false, remind: false, drive: false, location: false, storage: "small" },
  installedAt: 1,
};

describe("plugin permissions", () => {
  // §53: one line per permission the plugin asked for, each with whether it is on.
  it("lists one switch per permission a plugin asks for", () => {
    expect(permissionsOf(BOARD).map((one) => [one.key, one.on])).toEqual([
      ["messages", false],
      ["network:api.example.com", false],
      ["send", false],
      ["live", false],
      ["remind", false],
      ["drive", false],
      ["location", false],
      ["storage", false],
    ]);
    expect(permissionsOf({ ...BOARD, asks: { network: [], messages: false, send: "nothing" } })).toEqual([]);
  });

  it("changes one permission and leaves the rest as they were", () => {
    const live = withPermission(BOARD, "live", true);
    expect(live).toEqual({ ...BOARD.granted, live: true, print: undefined });
    expect(withPermission(BOARD, "send", true).send).toBe("propose");
    expect(withPermission({ ...BOARD, granted: { ...BOARD.granted, send: "propose" } }, "send", false).send).toBe("nothing");
    expect(withPermission(BOARD, "network:api.example.com", true).network).toEqual(["api.example.com"]);
    expect(withPermission(BOARD, "storage", true).storage).toBe("large");
    // 2026-10-02 (app#32): the phone's position has a switch of its own, and the others keep it.
    expect(withPermission(BOARD, "location", true).location).toBe(true);
    expect(withPermission({ ...BOARD, granted: { ...BOARD.granted, location: true } }, "live", true).location).toBe(true);
  });

  // A game talks to the same game on the other phone: the switch says so, not "plugin".
  it("names the live channel of a game as a game's, and a tool's as before", () => {
    const live = (plugin: PluginView) => permissionsOf(plugin).find((one) => one.key === "live")?.label;
    expect(live({ ...BOARD, kind: "game" })).toBe("Talk to the same game on the other person's phone");
    expect(live(BOARD)).toBe("Talk to the same plugin on the other side of the chat");
    expect(live({ ...BOARD, kind: "tool" })).toBe("Talk to the same plugin on the other side of the chat");
  });

  // A game never learns where the phone is: its switches never offer it, whatever it says it asks.
  it("never offers a game the phone's position", () => {
    const keys = permissionsOf({ ...BOARD, kind: "game" }).map((one) => one.key);
    expect(keys).not.toContain("location");
    expect(permissionsOf(BOARD).map((one) => one.key)).toContain("location");
  });

  // Ioan, 2026-10-08: a tool that lacks a permission it asked for asks the user on the spot. What
  // it lacks is read from the same lines as the switches, never from what the core said.
  it("says which permission a plugin asked for and lacks, with its label and icon", () => {
    expect(missingPermission(BOARD, "location")).toMatchObject({ key: "location", label: "Your location, only when you ask", on: false });
    expect(missingPermission(BOARD, "location")?.icon).toBeTruthy();
    expect(missingPermission(BOARD, "send")?.label).toBe("Write in the chat");
    expect(missingPermission(BOARD, "network:api.example.com")?.label).toBe("api.example.com");
    // Granted already: nothing to ask.
    expect(missingPermission({ ...BOARD, granted: { ...BOARD.granted, location: true } }, "location")).toBeUndefined();
    // Never asked for in its manifest: nothing the user could grant, so nothing to ask.
    expect(missingPermission({ ...BOARD, asks: { ...BOARD.asks, location: false } }, "location")).toBeUndefined();
    expect(missingPermission(BOARD, "network:elsewhere.example.com")).toBeUndefined();
    // A game is never offered the phone's position.
    expect(missingPermission({ ...BOARD, kind: "game" }, "location")).toBeUndefined();
  });

  // The host a plugin's fetch goes to, as the core reads it (`host_of` in ft-core's web.rs).
  it("reads the host of an address as the core does", () => {
    expect(hostOf("https://API.example.com:8443/v1?q=1")).toBe("api.example.com");
    expect(hostOf("https://api.example.com#top")).toBe("api.example.com");
    expect(hostOf("http://api.example.com/")).toBeUndefined();
    expect(hostOf("https://user@api.example.com/")).toBeUndefined();
    expect(hostOf("https:///nothing")).toBeUndefined();
  });
});
