import { describe, expect, it } from "vitest";
import handlers from "../src-tauri/src/lib.rs?raw";

const sources = import.meta.glob(["./**/*.ts", "./**/*.vue", "!./**/*.test.ts", "!./__tests__/**"], {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

function invoked(source: string): string[] {
  return [...source.matchAll(/invoke(?:<[^>]*>)?\(\s*"(core_[a-z0-9_]+)"/g)].map((found) => found[1]);
}

function registered(lib: string): Set<string> {
  const list = lib.slice(lib.indexOf("generate_handler!["), lib.indexOf("])", lib.indexOf("generate_handler![")));
  return new Set([...list.matchAll(/::(core_[a-z0-9_]+)/g)].map((found) => found[1]));
}

describe("the core's commands", () => {
  it("answers every command the app invokes", () => {
    const answered = registered(handlers);
    expect(answered.size).toBeGreaterThan(100);
    const asked = new Set(Object.values(sources).flatMap(invoked));
    expect(asked).toContain("core_plugin_location");
    expect([...asked].filter((command) => !answered.has(command)).sort()).toEqual([]);
  });
});
