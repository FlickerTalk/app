import { describe, expect, it } from "vitest";
import { i18n, t } from "./i18n";
import en from "./i18n/en.json";

function leaves(node: unknown, path: string[] = []): [string, unknown][] {
  if (typeof node !== "object" || node === null) return [[path.join("."), node]];
  return Object.entries(node).flatMap(([key, value]) => leaves(value, [...path, key]));
}

describe("i18n", () => {
  it("serves English, the source language (no translations in phase 1)", () => {
    expect(i18n.global.locale.value).toBe("en");
    expect(t("tabs.chats")).toBe("Chats");
  });

  it("has no empty entries in the catalogue", () => {
    for (const [key, value] of leaves(en)) {
      expect(typeof value, key).toBe("string");
      expect(String(value).trim(), key).not.toBe("");
    }
  });
});
