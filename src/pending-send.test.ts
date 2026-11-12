import { describe, expect, it } from "vitest";
import { offerSend, takeSend, type Proposal } from "./pending-send";

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
