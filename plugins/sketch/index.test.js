// The plugin's own tests (Plan §53): the line the finger leaves, and what the drawing is called.
import { describe, expect, it } from "vitest";
import { pathOf, penName, strokeAt } from "./dist/index.js";

describe("sketch", () => {
  // A line drawn through the middle of each pair of points is round where the finger was quick.
  it("rounds the line between the points of a stroke", () => {
    const path = pathOf([
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 10, y: 10 },
    ]);
    expect(path[0]).toEqual({ to: { x: 0, y: 0 } });
    expect(path[1]).toEqual({ control: { x: 10, y: 0 }, to: { x: 10, y: 5 } });
    expect(path[path.length - 1]).toEqual({ control: { x: 10, y: 10 }, to: { x: 10, y: 10 } });
  });

  it("is a dot when the finger did not move", () => {
    expect(pathOf([{ x: 4, y: 4 }])).toEqual([{ to: { x: 4, y: 4 } }]);
    expect(pathOf([])).toEqual([]);
  });

  it("finds a point in the drawing from where the finger is", () => {
    const box = { left: 100, top: 50, width: 200 };
    expect(strokeAt({ clientX: 200, clientY: 100 }, box, 400)).toEqual({ x: 200, y: 100 });
  });

  it("names the drawing after the day it was made", () => {
    expect(penName(new Date(2026, 8, 22, 16, 5, 9))).toBe("sketch-20260922-160509.png");
  });
});
