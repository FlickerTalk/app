import { describe, expect, it } from "vitest";
import { askSearch, takeSearch } from "./pending-search";

// Ioan, 2026-10-06: the contact page's search goes back to the conversation it came from, and the
// conversation opens its search when it is on screen again. The request is taken once.
describe("pending search", () => {
  it("is taken once, and only by the conversation it is for", () => {
    askSearch("c1");
    expect(takeSearch("c2")).toBe(false);
    expect(takeSearch("c1")).toBe(true);
    expect(takeSearch("c1")).toBe(false);
  });

  it("keeps only the latest request", () => {
    askSearch("c1");
    askSearch("c2");
    expect(takeSearch("c1")).toBe(false);
    expect(takeSearch("c2")).toBe(true);
  });
});
