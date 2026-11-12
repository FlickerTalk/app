import { describe, expect, it } from "vitest";
import { setLocale } from "../i18n";
import { clock, formatSize, hueOf } from "./format";

describe("a contact's colour", () => {
  it("comes from the id alone, so every phone shows the same", () => {
    expect([hueOf(""), hueOf("ft_bob"), hueOf("ft_carol")]).toEqual([61, 331, 149]);
  });
});

describe("the time of a message", () => {
  it("is hours and minutes with two digits each", () => {
    const at = (hours: number, minutes: number) => new Date(2026, 0, 1, hours, minutes).getTime();
    expect([at(0, 0), at(7, 5), at(23, 59)].map(clock)).toEqual(["00:00", "07:05", "23:59"]);
  });

  it("is empty when unknown", () => {
    expect(clock(0)).toBe("");
  });
});

describe("a file's size", () => {
  it("counts in thousands, with one decimal only below ten", () => {
    expect([999, 1_000, 2_345, 9_999, 12_345].map((bytes) => formatSize(bytes))).toEqual(["999 B", "1 KB", "2.3 KB", "10 KB", "12 KB"]);
  });

  it("stops at terabytes", () => {
    expect(formatSize(5_000_000_000_000_000)).toBe("5,000 TB");
  });

  it("writes the number the way the app's language does", async () => {
    await setLocale("de");
    try {
      expect([2_345, 5_000_000_000_000_000].map((bytes) => formatSize(bytes))).toEqual(["2,3 KB", "5.000 TB"]);
    } finally {
      await setLocale("en");
    }
  });
});
