import { describe, expect, it } from "vitest";
import { askSearch, inPane, showInPane, takeSearch } from "./pending-search";

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

  // QA of 1.4.0 (2026-10-06): on a tablet the conversation is in the chats tab's pane, not on a
  // page of its own; the contact page asks which one the pane shows.
  it("knows which conversation the chats tab shows beside its list", () => {
    expect(inPane("c1")).toBe(false);
    showInPane("c1");
    expect(inPane("c1")).toBe(true);
    expect(inPane("c2")).toBe(false);
    showInPane("");
    expect(inPane("c1")).toBe(false);
    expect(inPane("")).toBe(false);
  });
});
