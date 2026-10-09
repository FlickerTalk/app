import { describe, expect, it } from "vitest";
import { callScreenGone, callScreenMounted } from "./call-screen";

const settled = async (promise: Promise<void>) => {
  let done = false;
  void promise.then(() => (done = true));
  await Promise.resolve();
  await Promise.resolve();
  return done;
};

// The call screen is on the page from its mount to its unmount, its leaving transition included
// (2026-10-09): going back to the call before the old screen had gone left a blank page.
describe("call screen", () => {
  it("is gone at once when no call screen is on the page", async () => {
    expect(await settled(callScreenGone())).toBe(true);
  });

  it("is gone only once the call screen on the page unmounts", async () => {
    const unmounted = callScreenMounted();
    const gone = callScreenGone();
    expect(await settled(gone)).toBe(false);
    unmounted();
    expect(await settled(gone)).toBe(true);
  });

  it("waits for every call screen on the page, and counts each unmount once", async () => {
    const first = callScreenMounted();
    const second = callScreenMounted();
    first();
    first();
    expect(await settled(callScreenGone())).toBe(false);
    second();
    expect(await settled(callScreenGone())).toBe(true);
  });
});
