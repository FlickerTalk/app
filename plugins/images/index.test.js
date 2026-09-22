// The plugin's own tests (Plan §53): the arithmetic of the tool, away from any canvas.
import { describe, expect, it } from "vitest";
import { fitted, normalRect, outName } from "./dist/index.js";

describe("image tools", () => {
  it("makes a picture fit the longest side asked for", () => {
    expect(fitted({ width: 4000, height: 3000 }, 1280)).toEqual({ width: 1280, height: 960 });
    expect(fitted({ width: 3000, height: 4000 }, 1280)).toEqual({ width: 960, height: 1280 });
  });

  it("never makes a picture bigger than it is", () => {
    expect(fitted({ width: 800, height: 600 }, 1280)).toEqual({ width: 800, height: 600 });
    expect(fitted({ width: 800, height: 600 }, 0)).toEqual({ width: 800, height: 600 });
  });

  it("keeps at least one pixel", () => {
    expect(fitted({ width: 4000, height: 3 }, 10)).toEqual({ width: 10, height: 1 });
  });

  // A crop is dragged in any direction, and can be dragged outside the picture.
  it("turns a drag into a box inside the picture", () => {
    expect(normalRect({ x: 30, y: 40 }, { x: 10, y: 10 }, { width: 100, height: 100 })).toEqual({
      x: 10,
      y: 10,
      width: 20,
      height: 30,
    });
    expect(normalRect({ x: -20, y: 50 }, { x: 500, y: 500 }, { width: 100, height: 100 })).toEqual({
      x: 0,
      y: 50,
      width: 100,
      height: 50,
    });
  });

  it("is no crop at all when the drag went nowhere", () => {
    expect(normalRect({ x: 10, y: 10 }, { x: 10, y: 12 }, { width: 100, height: 100 })).toBe(null);
  });

  it("names what it made after what it was given", () => {
    expect(outName("holidays.HEIC", "jpg")).toBe("holidays.jpg");
    expect(outName("no dots", "jpg")).toBe("no dots.jpg");
    expect(outName("", "jpg")).toBe("image.jpg");
    expect(outName("../../etc/passwd", "jpg")).toBe("passwd.jpg");
  });

  it("is a custom element the frame can show", () => {
    expect(customElements.get("ft-images")).toBeTruthy();
  });
});
