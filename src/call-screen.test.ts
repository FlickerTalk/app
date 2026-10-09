import { afterEach, describe, expect, it, vi } from "vitest";
import { callScreenGone, callScreenMounted, showCallScreen } from "./call-screen";

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

/**
 * A router as the app's over the browser's history: each page has its position in it
 * (`history.state.position`, as vue-router keeps it), and the router says where it landed.
 */
function fakeRouter(start: string) {
  const stack = [start];
  let at = 0;
  const after: Array<(to: { path: string }) => void> = [];
  const currentRoute = { value: { path: start } };
  const land = () => {
    currentRoute.value.path = stack[at];
    window.history.replaceState({ position: at }, "");
    [...after].forEach((hook) => hook({ path: stack[at] }));
  };
  window.history.replaceState({ position: 0 }, "");
  return {
    stack: () => stack.slice(0, at + 1),
    currentRoute,
    afterEach: (hook: (to: { path: string }) => void) => {
      after.push(hook);
      return () => after.splice(after.indexOf(hook), 1);
    },
    push: vi.fn(async (path: string) => {
      stack.splice(at + 1, stack.length, path);
      at += 1;
      land();
    }),
    replace: vi.fn(async (path: string) => {
      stack.splice(at, stack.length, path);
      land();
    }),
    back: vi.fn(() => setTimeout(() => ((at -= 1), land()))),
  };
}

// 2026-10-09 (seen on the phones): with the call screen in the history under pages opened over it
// (a reminder's notification tapped during the call), going back to the call pushed a second call
// screen while the first was still mounted under them.
describe("going back to the call screen", () => {
  let unmount: Array<() => void> = [];
  afterEach(() => {
    unmount.forEach((gone) => gone());
    unmount = [];
  });

  /** The router lands on the call screen, and the screen mounts there. */
  async function onCallScreen(router: ReturnType<typeof fakeRouter>, path: string) {
    await router.push(path);
    unmount.push(callScreenMounted(router as never));
  }

  it("goes back to the call screen under the pages opened over it, pushing none", async () => {
    const router = fakeRouter("/chat/c1");
    await onCallScreen(router, "/call/c1");
    await router.push("/plugin/a");
    await router.push("/plugin/b");
    await showCallScreen(router as never, "/call/c1");
    // One page at a time (Ionic leaves mounted the pages a jump of several steps goes over).
    expect(router.back).toHaveBeenCalledTimes(2);
    expect(router.push).toHaveBeenCalledTimes(3);
    expect(router.stack()).toEqual(["/chat/c1", "/call/c1"]);
  });

  // The call shows itself on each of its events (answered, then connected, 2026-10-09): asked again
  // on the way, it was gone back to twice, past it.
  it("goes back to it once when asked again on the way there", async () => {
    const router = fakeRouter("/chat/c1");
    await onCallScreen(router, "/call/c1");
    await router.push("/plugin/a");
    await Promise.all([showCallScreen(router as never, "/call/c1"), showCallScreen(router as never, "/call/c1")]);
    expect(router.back).toHaveBeenCalledTimes(1);
    expect(router.stack()).toEqual(["/chat/c1", "/call/c1"]);
  });

  it("does nothing on the call screen itself", async () => {
    const router = fakeRouter("/chat/c1");
    await onCallScreen(router, "/call/c1");
    await showCallScreen(router as never, "/call/c1");
    expect(router.push).toHaveBeenCalledTimes(1);
    expect(router.back).not.toHaveBeenCalled();
  });

  it("pushes a call screen when there is none", async () => {
    const router = fakeRouter("/chat/c1");
    await showCallScreen(router as never, "/call/c1");
    expect(router.push).toHaveBeenCalledWith("/call/c1");
    expect(router.stack()).toEqual(["/chat/c1", "/call/c1"]);
  });

  it("pushes a new one once the call screen gone back from has unmounted, never going forward to it", async () => {
    const router = fakeRouter("/chat/c1");
    await onCallScreen(router, "/call/c1");
    router.back();
    await vi.waitFor(() => expect(router.currentRoute.value.path).toBe("/chat/c1"));
    let shown = false;
    const showing = showCallScreen(router as never, "/call/c1").then(() => (shown = true));
    await settled(Promise.resolve());
    expect(router.push).toHaveBeenCalledTimes(1);
    unmount.pop()?.();
    await showing;
    expect(shown).toBe(true);
    expect(router.back).toHaveBeenCalledTimes(1);
    expect(router.stack()).toEqual(["/chat/c1", "/call/c1"]);
  });

  // Gone back from, and a page pushed in its place before Ionic took it away: it is not in the
  // history any more, though its position is behind the page on top.
  it("does not go back to a call screen that left the history and is still leaving", async () => {
    const router = fakeRouter("/chat/c1");
    await onCallScreen(router, "/call/c1");
    router.back();
    await vi.waitFor(() => expect(router.currentRoute.value.path).toBe("/chat/c1"));
    await router.push("/plugin/a");
    await router.push("/plugin/b");
    const showing = showCallScreen(router as never, "/call/c1");
    await settled(Promise.resolve());
    expect(router.back).toHaveBeenCalledTimes(1);
    unmount.pop()?.();
    await showing;
    expect(router.stack()).toEqual(["/chat/c1", "/plugin/a", "/plugin/b", "/call/c1"]);
  });

  it("does not wait for another call's screen left under the pages on top", async () => {
    const router = fakeRouter("/chat/c2");
    await onCallScreen(router, "/call/c2");
    await router.push("/chat/c1");
    await showCallScreen(router as never, "/call/c1");
    expect(router.back).not.toHaveBeenCalled();
    expect(router.stack()).toEqual(["/chat/c2", "/call/c2", "/chat/c1", "/call/c1"]);
  });
});
