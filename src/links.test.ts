import { describe, expect, it } from "vitest";
import { piecesOf } from "./links";

describe("links in a message", () => {
  it("finds a web address and leaves the rest as text", () => {
    expect(piecesOf("mira https://flickertalk.com/add y dime")).toEqual([
      { kind: "text", text: "mira " },
      { kind: "link", text: "https://flickertalk.com/add", href: "https://flickertalk.com/add" },
      { kind: "text", text: " y dime" },
    ]);
  });

  it("finds an email address", () => {
    expect(piecesOf("escribe a info@flickertalk.com")).toEqual([
      { kind: "text", text: "escribe a " },
      { kind: "link", text: "info@flickertalk.com", href: "mailto:info@flickertalk.com" },
    ]);
  });

  it("leaves the punctuation out of the link", () => {
    expect(piecesOf("aquí: https://flickertalk.com/faq.")).toEqual([
      { kind: "text", text: "aquí: " },
      { kind: "link", text: "https://flickertalk.com/faq", href: "https://flickertalk.com/faq" },
      { kind: "text", text: "." },
    ]);
  });

  // Anything that is not a web or mail address stays as the text it was.
  it("never makes a link of something that could run", () => {
    expect(piecesOf("javascript:steal()")).toEqual([{ kind: "text", text: "javascript:steal()" }]);
    expect(piecesOf("file:///etc/passwd")).toEqual([{ kind: "text", text: "file:///etc/passwd" }]);
    expect(piecesOf("data:text/html,<script>")).toEqual([{ kind: "text", text: "data:text/html,<script>" }]);
  });

  it("is one piece of text when there is nothing to link", () => {
    expect(piecesOf("hola, qué tal")).toEqual([{ kind: "text", text: "hola, qué tal" }]);
  });
});
