import { describe, expect, it } from "vitest";

// The small picture of a video call settles into the nearest corner once let go (app#155), as the
// user manual says and calling apps do: never left half over the other person's face.
import { nearestCorner, thumbArea } from "./call-thumbnail";

/** A picture of 96 × 140 with its top-left at (x, y), by its edges. */
const picture = (x: number, y: number) => ({ left: x, top: y, right: x + 96, bottom: y + 140 });
/** Where it may settle on a 400 × 800 screen: 16 px from the sides, under the top buttons, over the call's. */
const area = { left: 16, top: 115, right: 384, bottom: 656 };

describe("the corner the small picture settles in", () => {
  it("goes to the corner of the quarter its centre is in", () => {
    // The area's middle is at (200, 385.5).
    expect(nearestCorner(picture(40, 150), area)).toEqual({ x: 16, y: 115 });
    expect(nearestCorner(picture(220, 150), area)).toEqual({ x: 288, y: 115 });
    expect(nearestCorner(picture(40, 400), area)).toEqual({ x: 16, y: 516 });
    expect(nearestCorner(picture(220, 400), area)).toEqual({ x: 288, y: 516 });
  });

  it("goes by the picture's centre, not its corner", () => {
    // Its top-left is in the left half, its centre (210) in the right one.
    expect(nearestCorner(picture(162, 150), area)).toEqual({ x: 288, y: 115 });
    // Its top is above the middle, its centre (395.5) below.
    expect(nearestCorner(picture(40, 325.5), area)).toEqual({ x: 16, y: 516 });
  });

  it("stays where it is when it is in a corner already", () => {
    expect(nearestCorner(picture(288, 516), area)).toEqual({ x: 288, y: 516 });
    expect(nearestCorner(picture(16, 115), area)).toEqual({ x: 16, y: 115 });
  });

  it("goes down and to the right from dead in the middle", () => {
    // Centre at (200, 385.5): on both middle lines.
    expect(nearestCorner(picture(152, 315.5), area)).toEqual({ x: 288, y: 516 });
  });

  it("sits inside the area's edges, wherever the area starts", () => {
    const inset = { left: 60, top: 200, right: 340, bottom: 600 };
    expect(nearestCorner(picture(0, 0), inset)).toEqual({ x: 60, y: 200 });
    expect(nearestCorner(picture(300, 700), inset)).toEqual({ x: 244, y: 460 });
  });

  it("comes back from outside the area", () => {
    expect(nearestCorner(picture(-50, -60), area)).toEqual({ x: 16, y: 115 });
    expect(nearestCorner(picture(390, 760), area)).toEqual({ x: 288, y: 516 });
  });
});

describe("where the small picture may settle", () => {
  const screen = { left: 0, top: 0, right: 400, bottom: 800 };

  it("keeps its starting corner's margin on both sides, its bottom over the call's buttons", () => {
    // Where it starts: 16 px from the right, its bottom 144 px over the screen's.
    const home = picture(288, 516);
    expect(thumbArea(screen, home, 99)).toEqual({ left: 16, top: 115, right: 384, bottom: 656 });
  });

  it("measures the margin from the side it starts on, the left one right to left", () => {
    const home = picture(16, 516);
    expect(thumbArea(screen, home, 99)).toEqual({ left: 16, top: 115, right: 384, bottom: 656 });
  });

  it("follows the screen where it is", () => {
    const shifted = { left: 20, top: 30, right: 420, bottom: 830 };
    expect(thumbArea(shifted, picture(308, 546), 129)).toEqual({ left: 36, top: 145, right: 404, bottom: 686 });
  });
});
