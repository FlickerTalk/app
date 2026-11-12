import { describe, expect, it, onTestFinished, vi } from "vitest";
import { effectScope, nextTick, ref } from "vue";

import { nearestCorner, thumbArea, useThumbDrag } from "./call-thumbnail";

const picture = (x: number, y: number) => ({ left: x, top: y, right: x + 96, bottom: y + 140 });
const area = { left: 16, top: 115, right: 384, bottom: 656 };

describe("the corner the small picture settles in", () => {
  it("goes to the corner of the quarter its centre is in", () => {
    expect(nearestCorner(picture(40, 150), area)).toEqual({ x: 16, y: 115 });
    expect(nearestCorner(picture(220, 150), area)).toEqual({ x: 288, y: 115 });
    expect(nearestCorner(picture(40, 400), area)).toEqual({ x: 16, y: 516 });
    expect(nearestCorner(picture(220, 400), area)).toEqual({ x: 288, y: 516 });
  });

  it("goes by the picture's centre, not its corner", () => {
    expect(nearestCorner(picture(162, 150), area)).toEqual({ x: 288, y: 115 });
    expect(nearestCorner(picture(40, 325.5), area)).toEqual({ x: 16, y: 516 });
  });

  it("stays where it is when it is in a corner already", () => {
    expect(nearestCorner(picture(288, 516), area)).toEqual({ x: 288, y: 516 });
    expect(nearestCorner(picture(16, 115), area)).toEqual({ x: 16, y: 115 });
  });

  it("goes down and to the right from dead in the middle", () => {
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

describe("dragging the small picture", () => {
  const rect = (x: number, y: number, width: number, height: number) => () =>
    ({ x, y, width, height, left: x, top: y, right: x + width, bottom: y + height, toJSON: () => ({}) }) as DOMRect;
  const pointer = (type: string, x: number, y: number) => new PointerEvent(type, { clientX: x, clientY: y, pointerId: 1 });

  function setUp(small: "local" | "remote" = "local") {
    const body = ref<HTMLElement | null>(document.createElement("div"));
    const localSlot = ref<HTMLElement | null>(document.createElement("div"));
    const remoteSlot = ref<HTMLElement | null>(document.createElement("div"));
    const localThumb = ref(small === "local");
    const remoteThumb = ref(small === "remote");
    const moved = vi.fn();
    const scope = effectScope();
    const drag = scope.run(() =>
      useThumbDrag({ body, localSlot, remoteSlot, localThumb, remoteThumb, above: () => [{ x: 8, y: 55, width: 44, height: 44 }], moved }),
    )!;
    onTestFinished(() => {
      drag.endDrag();
      scope.stop();
    });
    for (const slot of [localSlot.value!, remoteSlot.value!]) {
      slot.addEventListener("pointerdown", drag.grabThumb);
      slot.addEventListener("pointermove", drag.moveThumb);
      slot.addEventListener("pointerup", drag.dropThumb);
    }
    return { body, localSlot, remoteSlot, localThumb, moved, drag };
  }

  it("follows the finger from where it took the small picture", () => {
    const { localSlot, moved, drag } = setUp();
    localSlot.value!.dispatchEvent(pointer("pointerdown", 300, 600));
    expect(drag.dragging.value).toBe(true);
    window.dispatchEvent(pointer("pointermove", 200, 500));
    expect(drag.thumbStyle.value).toEqual({ transform: "translate(-100px, -100px)" });
    expect(moved).toHaveBeenCalled();
  });

  it("does not take a picture that is not the small one", () => {
    const { remoteSlot, drag } = setUp("local");
    remoteSlot.value!.dispatchEvent(pointer("pointerdown", 300, 600));
    expect(drag.dragging.value).toBe(false);
  });

  it("settles it in the nearest corner, under what sits above it, once let go", () => {
    const query = { matches: true, media: "(prefers-reduced-motion: reduce)", addEventListener: vi.fn(), removeEventListener: vi.fn() };
    vi.spyOn(window, "matchMedia").mockImplementation((media) => ({ ...query, matches: media === query.media }) as unknown as MediaQueryList);
    onTestFinished(() => vi.mocked(window.matchMedia).mockRestore());
    const { body, localSlot, drag } = setUp();
    body.value!.getBoundingClientRect = rect(0, 0, 400, 800);
    localSlot.value!.getBoundingClientRect = rect(88, 266, 96, 140);
    localSlot.value!.dispatchEvent(pointer("pointerdown", 330, 580));
    window.dispatchEvent(pointer("pointermove", 130, 330));
    localSlot.value!.dispatchEvent(pointer("pointerup", 130, 330));
    expect(drag.dragging.value).toBe(false);
    expect(drag.thumbStyle.value).toEqual({ transform: "translate(-272px, -401px)" });
  });

  it("forgets a drag cut short when the small picture changes", async () => {
    const { localSlot, localThumb, drag } = setUp();
    localSlot.value!.dispatchEvent(pointer("pointerdown", 300, 600));
    window.dispatchEvent(pointer("pointermove", 200, 500));
    localThumb.value = false;
    await nextTick();
    expect(drag.dragging.value).toBe(false);
    localThumb.value = true;
    await nextTick();
    expect(drag.thumbStyle.value).toBeUndefined();
  });
});
