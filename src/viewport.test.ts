import { afterEach, beforeEach, describe, expect, it } from "vitest";

// The on-screen keyboard (2026-09-29, seen on the iPhone 13 mini, the Lenovo tablet and the
// Samsung S20+): neither WebView shrinks the page for the keyboard. Only the visual viewport
// shrinks, and the WebView pans it to show the focused field, which pushed the chat's header off
// the screen. The app follows the visual viewport instead: it is as tall as what can be seen.
import { fitViewport, isAtEnd, startViewportFit, stickToEnd, watchViewport } from "./viewport";

describe("fitting the app to the visible area", () => {
  it("takes the whole page while no keyboard is up", () => {
    expect(fitViewport({ layoutHeight: 952, height: 952, offsetTop: 0 })).toEqual({
      height: 952,
      top: 0,
      keyboard: 0,
      open: false,
    });
  });

  it("shrinks to what the keyboard leaves and keeps the top of the page on screen", () => {
    // Measured on the Android 16 emulator: the WebView panned 336 px to show the composer.
    expect(fitViewport({ layoutHeight: 952, height: 616, offsetTop: 336 })).toEqual({
      height: 616,
      top: 336,
      keyboard: 336,
      open: true,
    });
    expect(fitViewport({ layoutHeight: 952, height: 616, offsetTop: 0 })).toMatchObject({ top: 0, keyboard: 336 });
  });

  it("rounds to whole pixels and never leaves the page", () => {
    expect(fitViewport({ layoutHeight: 800, height: 431.6, offsetTop: 368.4 })).toEqual({
      height: 432,
      top: 368,
      keyboard: 368,
      open: true,
    });
    // iOS can overshoot while it scrolls the focused field into view.
    expect(fitViewport({ layoutHeight: 800, height: 432, offsetTop: 500 }).top).toBe(368);
    expect(fitViewport({ layoutHeight: 800, height: 432, offsetTop: -12 }).top).toBe(0);
    expect(fitViewport({ layoutHeight: 800, height: 900, offsetTop: 0 }).height).toBe(800);
  });

  it("does not take a small change for a keyboard", () => {
    // iOS's shortcut bar over a hardware keyboard, or a browser bar that folds.
    expect(fitViewport({ layoutHeight: 852, height: 797, offsetTop: 0 })).toMatchObject({ keyboard: 55, open: false });
    expect(fitViewport({ layoutHeight: 852, height: 552, offsetTop: 0 }).open).toBe(true);
  });
});

describe("staying at the end of a conversation", () => {
  it("counts as at the end what is within a few pixels of it", () => {
    expect(isAtEnd({ scrollHeight: 2000, clientHeight: 800, scrollTop: 1200 })).toBe(true);
    expect(isAtEnd({ scrollHeight: 2000, clientHeight: 800, scrollTop: 1180 })).toBe(true);
    expect(isAtEnd({ scrollHeight: 2000, clientHeight: 800, scrollTop: 900 })).toBe(false);
  });

  it("is at the end when everything fits", () => {
    expect(isAtEnd({ scrollHeight: 500, clientHeight: 800, scrollTop: 0 })).toBe(true);
  });
});

/** A visual viewport the test moves, as the keyboard would. */
class FakeViewport extends EventTarget {
  height = 952;
  offsetTop = 0;
  move(height: number, offsetTop = 0) {
    this.height = height;
    this.offsetTop = offsetTop;
    this.dispatchEvent(new Event("resize"));
  }
}

describe("following the visual viewport", () => {
  let viewport: FakeViewport;
  let stop: () => void;
  const root = document.documentElement;
  const events: string[] = [];
  const record = (event: Event) => {
    const height = (event as CustomEvent<{ keyboardHeight?: number }>).detail?.keyboardHeight;
    events.push(height === undefined ? event.type : `${event.type}:${height}`);
  };

  beforeEach(() => {
    viewport = new FakeViewport();
    Object.defineProperty(window, "visualViewport", { value: viewport, configurable: true });
    Object.defineProperty(window, "innerHeight", { value: 952, configurable: true });
    events.length = 0;
    window.addEventListener("keyboardWillShow", record);
    window.addEventListener("keyboardWillHide", record);
    stop = startViewportFit(window, root);
  });

  afterEach(() => {
    stop();
    window.removeEventListener("keyboardWillShow", record);
    window.removeEventListener("keyboardWillHide", record);
    root.removeAttribute("style");
    root.classList.remove("ft-keyboard-open");
  });

  it("sizes the app to the visible area from the start", () => {
    expect(root.style.getPropertyValue("--ft-viewport-height")).toBe("952px");
    expect(root.style.getPropertyValue("--ft-viewport-top")).toBe("0px");
    expect(root.classList.contains("ft-keyboard-open")).toBe(false);
  });

  it("follows the keyboard up and down, and tells Ionic's footer and tab bar", () => {
    viewport.move(616, 336);
    expect(root.style.getPropertyValue("--ft-viewport-height")).toBe("616px");
    expect(root.style.getPropertyValue("--ft-viewport-top")).toBe("336px");
    expect(root.classList.contains("ft-keyboard-open")).toBe(true);

    // The WebView scrolls back once the field fits: only the position changes.
    viewport.offsetTop = 0;
    viewport.dispatchEvent(new Event("scroll"));
    expect(root.style.getPropertyValue("--ft-viewport-top")).toBe("0px");

    viewport.move(952, 0);
    expect(root.style.getPropertyValue("--ft-viewport-height")).toBe("952px");
    expect(root.classList.contains("ft-keyboard-open")).toBe(false);
    // Ionic's keyboard events (Capacitor's names), once per change, not once per frame.
    expect(events).toEqual(["keyboardWillShow:336", "keyboardWillHide"]);
  });

  it("lets a conversation note where it was before the size changes and act after", () => {
    const seen: string[] = [];
    const unwatch = watchViewport({
      before: () => seen.push(`before:${root.style.getPropertyValue("--ft-viewport-height")}`),
      after: () => seen.push(`after:${root.style.getPropertyValue("--ft-viewport-height")}`),
    });
    viewport.move(616, 0);
    // The position alone does not resize anything.
    viewport.offsetTop = 10;
    viewport.dispatchEvent(new Event("scroll"));
    unwatch();
    viewport.move(952, 0);
    expect(seen).toEqual(["before:952px", "after:616px"]);
  });

  it("keeps a conversation that showed its last message at its end when the keyboard opens", () => {
    // What a shrinking scroller does: same scrollTop, fewer pixels shown.
    const scroller = { scrollHeight: 2000, clientHeight: 800, scrollTop: 1200 };
    const unstick = stickToEnd(() => scroller);
    viewport.move(616, 0);
    scroller.clientHeight = 464;
    viewport.dispatchEvent(new Event("resize"));
    expect(scroller.scrollTop).toBe(2000);
    unstick();
  });

  it("leaves a conversation scrolled back in time where it was", () => {
    const scroller = { scrollHeight: 2000, clientHeight: 800, scrollTop: 300 };
    const unstick = stickToEnd(() => scroller);
    viewport.move(616, 0);
    expect(scroller.scrollTop).toBe(300);
    unstick();
    // Nothing to keep before the scroller exists.
    const none = stickToEnd(() => null);
    expect(() => viewport.move(952, 0)).not.toThrow();
    none();
  });

  it("stops following when asked", () => {
    stop();
    viewport.move(616, 336);
    expect(root.style.getPropertyValue("--ft-viewport-height")).toBe("952px");
  });

  it("does nothing without a visual viewport (an old desktop browser)", () => {
    stop();
    root.removeAttribute("style");
    Object.defineProperty(window, "visualViewport", { value: null, configurable: true });
    stop = startViewportFit(window, root);
    expect(root.style.getPropertyValue("--ft-viewport-height")).toBe("");
  });
});
