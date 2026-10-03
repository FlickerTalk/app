import { describe, expect, it } from "vitest";

// The avatar took `part[0]`, the first UTF-16 code unit, so a name starting with an emoji or with
// a character outside the Basic Multilingual Plane showed half a surrogate pair (app#71).
import { firstCharacter, initials } from "./initials";

describe("avatar initials", () => {
  it("takes the first letter of the first two words, upper-cased", () => {
    expect(initials("maria lópez garcía")).toBe("ML");
  });

  it("ignores extra spaces", () => {
    expect(initials("  Alex   Chen ")).toBe("AC");
  });

  it("is empty for an empty name", () => {
    expect(initials("   ")).toBe("");
  });

  it("keeps an emoji whole", () => {
    expect(initials("😀 Party")).toBe("😀P");
  });

  it("keeps a CJK name readable", () => {
    expect(initials("王小明")).toBe("王");
    expect(initials("𠮷田 太郎")).toBe("𠮷太");
  });

  it("keeps a grapheme cluster whole: flags and joined emoji", () => {
    expect(initials("🇪🇸 Spain")).toBe("🇪🇸S");
    expect(initials("👩‍👩‍👧 Family")).toBe("👩‍👩‍👧F");
  });
});

describe("first character of a word", () => {
  it("uses grapheme clusters when the platform can segment", () => {
    expect(firstCharacter("🇪🇸x", true)).toBe("🇪🇸");
  });

  it("falls back to whole code points without a segmenter", () => {
    expect(firstCharacter("😀x", false)).toBe("😀");
    expect(firstCharacter("𠮷田", false)).toBe("𠮷");
    expect(firstCharacter("ab", false)).toBe("a");
  });
});
