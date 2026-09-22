// The plugin's own tests (Plan §53): what is covered is covered in the pixels themselves, so the
// picture that leaves the phone no longer holds what was underneath.
import { describe, expect, it } from "vitest";
import { blocksOf, normalRect, outName } from "./dist/index.js";

describe("cover", () => {
  it("turns a drag into a box inside the picture", () => {
    expect(normalRect({ x: 60, y: 80 }, { x: 20, y: 20 }, { width: 100, height: 100 })).toEqual({
      x: 20,
      y: 20,
      width: 40,
      height: 60,
    });
  });

  it("ignores a tap that covers nothing", () => {
    expect(normalRect({ x: 10, y: 10 }, { x: 12, y: 12 }, { width: 100, height: 100 })).toBe(null);
  });

  // The mosaic is drawn block by block, so nothing of what was there survives in between.
  it("cuts a box into blocks that cover it whole", () => {
    const blocks = blocksOf({ x: 0, y: 0, width: 20, height: 10 }, 8);
    expect(blocks).toHaveLength(6);
    expect(blocks[0]).toEqual({ x: 0, y: 0, width: 8, height: 8 });
    const last = blocks[blocks.length - 1];
    expect(last.x + last.width).toBe(20);
    expect(last.y + last.height).toBe(10);
  });

  it("is one block when the box is smaller than one", () => {
    expect(blocksOf({ x: 5, y: 5, width: 4, height: 4 }, 16)).toEqual([{ x: 5, y: 5, width: 4, height: 4 }]);
  });

  it("names what it made after what it was given", () => {
    expect(outName("passport.png")).toBe("passport-covered.jpg");
    expect(outName("")).toBe("image-covered.jpg");
  });

  it("is a custom element the frame can show", () => {
    expect(customElements.get("ft-redact")).toBeTruthy();
  });
});
