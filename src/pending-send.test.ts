import { describe, expect, it } from "vitest";
import { offerSend, takeSend, type Proposal } from "./pending-send";

// Ioan, 2026-10-09: a tool opened on its own (from Apps, from a reminder) has no chat behind it.
// What it proposes waits here for the conversation the user picked, which puts it in its composer
// once it is on screen. Only the latest proposal counts, and it is taken once.
describe("pending send", () => {
  const text: Proposal = { kind: "text", text: "# Title" };
  const file: Proposal = { kind: "file", file: { path: "/data/files/outgoing/1-notes.md", name: "notes.md", mime: "text/markdown", size: 4 } };

  it("is taken once, and only by the conversation it is for", () => {
    offerSend("c1", text);
    expect(takeSend("c2")).toBeNull();
    expect(takeSend("c1")).toEqual(text);
    expect(takeSend("c1")).toBeNull();
  });

  it("keeps only the latest proposal", () => {
    offerSend("c1", text);
    offerSend("c2", file);
    expect(takeSend("c1")).toBeNull();
    expect(takeSend("c2")).toEqual(file);
  });

  it("has nothing for no conversation", () => {
    offerSend("", text);
    expect(takeSend("")).toBeNull();
  });
});
