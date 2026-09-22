import { beforeEach, describe, expect, it } from "vitest";
import { GROUPS, RECENT_LIMIT, recent, remember } from "./emoji";

describe("emoji", () => {
  beforeEach(() => localStorage.clear());

  it("offers groups, each with an emoji for its tab", () => {
    expect(GROUPS.length).toBeGreaterThan(4);
    for (const group of GROUPS) {
      expect(group.id).toMatch(/^[a-z]+$/);
      expect(group.emoji.length).toBeGreaterThan(8);
      expect(group.emoji).toContain(group.icon);
    }
  });

  // The picker is written by hand: a repeated emoji would be a mistake, not a choice.
  it("never repeats an emoji", () => {
    const all = GROUPS.flatMap((group) => group.emoji);
    expect(new Set(all).size).toBe(all.length);
  });

  it("has nothing recent until one is used", () => {
    expect(recent()).toEqual([]);
  });

  it("keeps the last used first, without repeating it", () => {
    remember("😀");
    remember("🎉");
    remember("😀");
    expect(recent()).toEqual(["😀", "🎉"]);
  });

  it("remembers only the last few", () => {
    for (const emoji of GROUPS[0].emoji.slice(0, RECENT_LIMIT + 3)) remember(emoji);
    expect(recent()).toHaveLength(RECENT_LIMIT);
  });

  it("survives a broken store", () => {
    localStorage.setItem("ft-recent-emoji", "{oops");
    expect(recent()).toEqual([]);
  });
});
