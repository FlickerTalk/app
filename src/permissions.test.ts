import { describe, expect, it } from "vitest";
import { permissionsOf, withPermission } from "./permissions";
import type { PluginView } from "./core";

const BOARD: PluginView = {
  id: "com.flickertalk.board",
  name: "Board",
  version: "1.0.0",
  asks: { network: ["api.example.com"], messages: true, send: "propose", live: true, remind: true, drive: true, storage: "large" },
  granted: { network: [], messages: false, send: "nothing", live: false, remind: false, drive: false, storage: "small" },
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
  });
});
