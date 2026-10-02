import { describe, expect, it } from "vitest";
import { permissionsOf, withPermission } from "./permissions";
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
});
