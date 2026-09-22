// The plugin's own test: what it makes of a message it is handed (Plan §53).
import { describe, expect, it } from "vitest";
import { readCode } from "./dist/index.js";

describe("code block", () => {
  it("reads a fenced block with its language", () => {
    expect(readCode("```rust\nfn main() {}\n```")).toEqual({ language: "rust", code: "fn main() {}" });
  });

  it("keeps the text as it is when there is no fence", () => {
    expect(readCode("just text")).toEqual({ language: "", code: "just text" });
  });

  it("ignores a language it does not know, and never runs it", () => {
    expect(readCode("```<script>evil</script>\ncode\n```")).toEqual({ language: "", code: "code" });
  });

  it("keeps the indentation of the code", () => {
    expect(readCode("```python\nif x:\n    go()\n```").code).toBe("if x:\n    go()");
  });
});
