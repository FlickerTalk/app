import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref, type Ref } from "vue";
import { startViewportFit } from "./viewport";
import { useTypingInFrame } from "./frameTyping";

class FakeViewport extends EventTarget {
  height = 900;
  offsetTop = 0;
  move(height: number) {
    this.height = height;
    this.dispatchEvent(new Event("resize"));
  }
}

enableAutoUnmount(afterEach);

describe("useTypingInFrame", () => {
  let viewport: FakeViewport;
  let stop: () => void;
  let frame: HTMLIFrameElement;
  let other: HTMLButtonElement;
  const root = document.documentElement;

  beforeEach(() => {
    vi.useFakeTimers();
    viewport = new FakeViewport();
    Object.defineProperty(window, "visualViewport", { value: viewport, configurable: true });
    Object.defineProperty(window, "innerHeight", { value: 900, configurable: true });
    Object.defineProperty(root, "clientHeight", { value: 900, configurable: true });
    stop = startViewportFit(window, root);
    frame = document.createElement("iframe");
    other = document.createElement("button");
    document.body.append(frame, other);
  });

  afterEach(() => {
    stop();
    frame.remove();
    other.remove();
    root.removeAttribute("style");
    root.classList.remove("ft-keyboard-open");
    vi.useRealTimers();
  });

  function watching(target: () => HTMLIFrameElement | null = () => frame, room: () => HTMLElement | null = () => null): Ref<boolean> {
    let typing!: Ref<boolean>;
    mount(
      defineComponent({
        setup() {
          typing = useTypingInFrame(target, room);
          return () => h("div");
        },
      }),
    );
    return typing;
  }

  function focusFrame() {
    frame.focus();
    window.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
  }

  it("is typing while the keyboard is up for a field inside the frame", () => {
    const typing = watching();
    focusFrame();
    expect(document.activeElement).toBe(frame);
    expect(typing.value).toBe(false);
    viewport.move(560);
    expect(typing.value).toBe(true);
  });

  it("stops when the keyboard goes", () => {
    const typing = watching();
    focusFrame();
    viewport.move(560);
    viewport.move(900);
    expect(typing.value).toBe(false);
  });

  it("stops when the focus moves to the app with the keyboard still up, and starts again when it goes back", () => {
    const typing = watching();
    focusFrame();
    viewport.move(560);
    other.focus();
    expect(typing.value).toBe(false);
    focusFrame();
    expect(typing.value).toBe(true);
  });

  it("is not typing when the keyboard is up for something of the app's own", () => {
    const typing = watching();
    other.focus();
    viewport.move(560);
    expect(typing.value).toBe(false);
  });

  it("is not typing without a frame, nor once it has gone", () => {
    const shown = ref<HTMLIFrameElement | null>(null);
    const typing = watching(() => shown.value);
    focusFrame();
    viewport.move(560);
    expect(typing.value).toBe(false);
  });

  describe("the field the frame focused", () => {
    const fieldAt = (top: number, bottom: number) => {
      const event = new MessageEvent("message", { data: { type: "ft.field", top, bottom } });
      Object.defineProperty(event, "source", { value: frame.contentWindow });
      window.dispatchEvent(event);
    };
    let room: HTMLElement;
    const scrolled = { top: 0 };

    beforeEach(() => {
      room = document.createElement("div");
      scrolled.top = 0;
      Object.defineProperty(room, "scrollTop", { get: () => scrolled.top, set: (top: number) => (scrolled.top = Math.max(0, top)), configurable: true });
      vi.spyOn(room, "getBoundingClientRect").mockReturnValue({ top: 156, bottom: 400 } as DOMRect);
      vi.spyOn(frame, "getBoundingClientRect").mockImplementation(() => ({ top: 156 - scrolled.top }) as DOMRect);
      Object.defineProperty(frame, "contentWindow", { value: {}, configurable: true });
    });

    afterEach(() => vi.restoreAllMocks());

    it("comes into view above the keyboard with a little room under it, once the app has made room", async () => {
      watching(() => frame, () => room);
      fieldAt(206, 254);
      focusFrame();
      viewport.move(400);
      expect(scrolled.top).toBe(0);
      await nextTick();
      expect(scrolled.top).toBe(254 + 156 - 400 + 8);
    });

    it("comes into view when the frame says where it is after the keyboard is up", async () => {
      watching(() => frame, () => room);
      focusFrame();
      viewport.move(400);
      await nextTick();
      fieldAt(420, 468);
      expect(scrolled.top).toBe(468 + 156 - 400 + 8);
      fieldAt(10, 58);
      expect(scrolled.top).toBe(10 - 8);
    });

    it("stays where it is when it is already in view, or when nobody is typing", async () => {
      watching(() => frame, () => room);
      fieldAt(420, 468);
      expect(scrolled.top).toBe(0);
      focusFrame();
      fieldAt(50, 98);
      viewport.move(400);
      await nextTick();
      expect(scrolled.top).toBe(0);
    });

    it("takes no place from another frame", () => {
      watching(() => frame, () => room);
      focusFrame();
      viewport.move(400);
      const event = new MessageEvent("message", { data: { type: "ft.field", top: 420, bottom: 468 } });
      Object.defineProperty(event, "source", { value: {} });
      window.dispatchEvent(event);
      expect(scrolled.top).toBe(0);
    });
  });
});
