import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref, type Ref } from "vue";
import { fitName, NAME_MIN_PX, useNameFit, type NameFit } from "./tile-name";

const scaled = (widthAt12: number) => (size: number) => (widthAt12 * size) / 12;

describe("fitName", () => {
  it("leaves a name alone when its widest word fits the tile", () => {
    expect(fitName(scaled(70), 80, 12)).toEqual({ clipped: false });
    expect(fitName(scaled(80), 80, 12)).toEqual({ clipped: false });
  });

  it("shrinks a word wider than the tile just enough to fit it", () => {
    expect(fitName(scaled(100), 80, 12)).toEqual({ size: 9.6, clipped: false });
  });

  it("rounds the size down, so the word never ends up wider than the tile", () => {
    const fit = fitName(scaled(97), 79.75, 12);
    expect(fit).toEqual({ size: 9.8, clipped: false });
    expect(scaled(97)(fit.size!)).toBeLessThanOrEqual(79.75);
  });

  it("measures again at the smaller size, since a font does not always narrow in step with its size", () => {
    const wider = (size: number) => (size < 12 ? scaled(88.5)(size) * 1.02 : 88.5);
    const fit = fitName(wider, 76, 12);
    expect(fit).toEqual({ size: 10.1, clipped: false });
    expect(wider(fit.size!)).toBeLessThanOrEqual(76);
  });

  it("stops at the smallest readable size and then cuts the name", () => {
    expect(NAME_MIN_PX).toBe(9);
    expect(fitName(scaled(108), 81, 12)).toEqual({ size: 9, clipped: false });
    expect(fitName(scaled(110), 82, 12)).toEqual({ size: NAME_MIN_PX, clipped: true });
    expect(fitName(scaled(200), 80, 12)).toEqual({ size: NAME_MIN_PX, clipped: true });
  });

  it("does nothing while the tile has no width yet", () => {
    expect(fitName(scaled(100), 0, 12)).toEqual({ clipped: false });
    expect(fitName(scaled(0), 0, 12)).toEqual({ clipped: false });
  });
});

describe("useNameFit", () => {
  let observers: Array<{ callback: () => void; watched: Element[]; disconnect: ReturnType<typeof vi.fn> }>;

  beforeEach(() => {
    observers = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        watched: Element[] = [];
        disconnect = vi.fn();
        constructor(public callback: () => void) {
          observers.push(this);
        }
        observe(element: Element) {
          this.watched.push(element);
        }
      },
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    document.body.innerHTML = "";
    document.head.querySelectorAll("style.base-size").forEach((css) => css.remove());
  });

  function wide(element: Element, width: number) {
    element.getBoundingClientRect = () => ({ width }) as DOMRect;
  }

  function sized(element: Element, widthAt12: number) {
    element.getBoundingClientRect = () => ({ width: scaled(widthAt12)(parseFloat((element as HTMLElement).style.fontSize) || 12) }) as DOMRect;
  }

  function tile() {
    const css = document.createElement("style");
    css.className = "base-size";
    css.textContent = ".probe { font-size: 12px; }";
    document.head.append(css);
    let fit!: Readonly<Ref<NameFit>>;
    const wrapper = mount(
      defineComponent({
        setup() {
          const box = ref<HTMLElement>();
          const probe = ref<HTMLElement>();
          fit = useNameFit(box, probe);
          return () => h("div", [h("span", { ref: box, class: "box" }), h("span", { ref: probe, class: "probe" })]);
        },
      }),
      { attachTo: document.body },
    );
    return { wrapper, fit: () => fit.value, box: wrapper.find(".box").element, probe: wrapper.find(".probe").element as HTMLElement };
  }

  it("watches the name's box and the width of its widest word", () => {
    const { box, probe } = tile();
    expect(observers).toHaveLength(1);
    expect(observers[0].watched).toEqual([box, probe]);
  });

  it("fits the name each time the tile or the name changes size", async () => {
    const { box, probe, fit } = tile();
    wide(box, 80);
    sized(probe, 60);
    observers[0].callback();
    expect(fit()).toEqual({ clipped: false });

    sized(probe, 100);
    observers[0].callback();
    await nextTick();
    expect(fit()).toEqual({ size: 9.6, clipped: false });
    expect(probe.style.fontSize).toBe("");

    wide(box, 120);
    observers[0].callback();
    expect(fit()).toEqual({ clipped: false });
  });

  it("stops watching when the tile goes away", () => {
    const { wrapper } = tile();
    wrapper.unmount();
    expect(observers[0].disconnect).toHaveBeenCalled();
  });

  it("leaves the name as it is where the browser cannot watch sizes", () => {
    vi.stubGlobal("ResizeObserver", undefined);
    const { fit } = tile();
    expect(fit()).toEqual({ clipped: false });
  });
});
