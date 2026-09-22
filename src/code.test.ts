import { describe, expect, it } from "vitest";
import { readCode } from "./code";

describe("code in a message", () => {
  it("reads a fenced block with its language", () => {
    expect(readCode("```rust\nfn main() {}\n```")).toEqual({ language: "rust", code: "fn main() {}" });
  });

  it("reads a fenced block without a language", () => {
    expect(readCode("```\nls -la\n```")).toEqual({ language: "", code: "ls -la" });
  });

  it("keeps the indentation, which is what code is about", () => {
    expect(readCode("```python\nif x:\n    go()\n```")?.code).toBe("if x:\n    go()");
  });

  it("ignores a language it does not know, and never treats it as markup", () => {
    expect(readCode("```<script>evil</script>\ncode\n```")).toEqual({ language: "", code: "code" });
  });

  it("is nothing for a message that is not code", () => {
    expect(readCode("just text")).toBeNull();
    expect(readCode("```not closed")).toBeNull();
    expect(readCode("")).toBeNull();
  });
});
