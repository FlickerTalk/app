import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon, IonSpinner } from "@ionic/vue";
import { addOutline, downloadOutline, imageOutline, lockClosedOutline } from "ionicons/icons";
import AppTile from "./AppTile.vue";
import source from "./AppTile.vue?raw";

const tile = (props: Record<string, unknown> = {}) =>
  mount(AppTile, { props: { name: "Image", icon: imageOutline, ...props }, attachTo: document.body });

const icons = (wrapper: ReturnType<typeof tile>) => wrapper.findAllComponents(IonIcon).map((one) => one.props("icon"));

describe("AppTile", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = "";
  });

  it("is a button with the app's icon and its name", () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    expect(button.exists()).toBe(true);
    expect(button.attributes("type")).toBe("button");
    expect(button.classes()).toContain("ion-activatable");
    expect(wrapper.find("ion-ripple-effect").exists()).toBe(true);
    expect(button.attributes("aria-label")).toBe("Image");
    expect(wrapper.find(".ft-app-tile__name").text()).toBe("Image");
    expect(icons(wrapper)).toEqual([imageOutline]);
    expect(wrapper.find(".ft-app-tile__badge").exists()).toBe(false);
    expect(wrapper.find(".ft-app-tile__caption").exists()).toBe(false);
  });

  it("wears a badge: a download, an app the phone carries, or the lock", () => {
    expect(icons(tile({ badge: "download" }))).toEqual([imageOutline, downloadOutline]);
    expect(icons(tile({ badge: "add" }))).toEqual([imageOutline, addOutline]);
    const locked = tile({ badge: "lock" });
    expect(icons(locked)).toEqual([imageOutline, lockClosedOutline]);
    expect(locked.find(".ft-app-tile__badge").classes()).toContain("ft-app-tile__badge--lock");
    expect(locked.find("[data-test='locked']").exists()).toBe(true);
    expect(tile({ badge: "download" }).find("[data-test='locked']").exists()).toBe(false);
    expect(locked.find("button").attributes("aria-label")).toBe("Image, locked");
  });

  it("says what a download weighs, under its name, dimmed while it is not installed", () => {
    const wrapper = tile({ off: true, badge: "download", caption: "48 KB" });
    expect(wrapper.find("button").classes()).toContain("ft-app-tile--off");
    expect(wrapper.find(".ft-app-tile__caption").text()).toBe("48 KB");
    expect(wrapper.find("button").attributes("aria-label")).toBe("Image, download 48 KB");
    expect(tile({ off: true, badge: "add" }).find("button").attributes("aria-label")).toBe("Image, add");
  });

  it("shows a spinner over its icon while busy, without growing", () => {
    const wrapper = tile({ busy: true, badge: "download" });
    const box = wrapper.find(".ft-app-tile__icon");
    expect(box.findComponent(IonSpinner).exists()).toBe(true);
    expect(box.findComponent(IonIcon).exists()).toBe(true);
    expect(wrapper.find("button").attributes("aria-busy")).toBe("true");
    expect(tile().findComponent(IonSpinner).exists()).toBe(false);
  });

  it("ignores taps and holds while busy, without being disabled or keeping a pressed look", async () => {
    const wrapper = tile({ busy: true });
    const button = wrapper.find("button");
    expect(button.attributes("disabled")).toBeUndefined();
    expect(button.attributes("aria-disabled")).toBe("true");
    expect(button.classes()).not.toContain("ion-activatable");
    expect(wrapper.find("ion-ripple-effect").exists()).toBe(false);
    await button.trigger("click");
    button.element.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    await button.trigger("pointerdown", { button: 0, clientX: 1, clientY: 1 });
    vi.advanceTimersByTime(1000);
    expect(wrapper.emitted("tap")).toBeUndefined();
    expect(wrapper.emitted("hold")).toBeUndefined();
    await wrapper.setProps({ busy: false });
    expect(button.classes()).toContain("ion-activatable");
    await button.trigger("click");
    expect(wrapper.emitted("tap")).toHaveLength(1);
  });

  it("taps", async () => {
    const wrapper = tile();
    await wrapper.find("button").trigger("click");
    expect(wrapper.emitted("tap")).toHaveLength(1);
    expect(wrapper.emitted("hold")).toBeUndefined();
  });

  it("holds after half a second of pressing, and then does not tap", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    await button.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    vi.advanceTimersByTime(499);
    expect(wrapper.emitted("hold")).toBeUndefined();
    vi.advanceTimersByTime(1);
    expect(wrapper.emitted("hold")).toHaveLength(1);
    await button.trigger("pointerup", { button: 0, clientX: 10, clientY: 10 });
    await button.trigger("click");
    expect(wrapper.emitted("tap")).toBeUndefined();
    await button.trigger("click");
    expect(wrapper.emitted("tap")).toHaveLength(1);
  });

  it("does not hold a press let go too soon, and that one taps", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    await button.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    vi.advanceTimersByTime(300);
    await button.trigger("pointerup", { button: 0, clientX: 10, clientY: 10 });
    vi.advanceTimersByTime(1000);
    await button.trigger("click");
    expect(wrapper.emitted("hold")).toBeUndefined();
    expect(wrapper.emitted("tap")).toHaveLength(1);
  });

  it("does not hold when the finger moves more than 10 px, or the press is cancelled", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    await button.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    await button.trigger("pointermove", { clientX: 15, clientY: 16 });
    vi.advanceTimersByTime(200);
    await button.trigger("pointermove", { clientX: 10, clientY: 22 });
    vi.advanceTimersByTime(1000);
    expect(wrapper.emitted("hold")).toBeUndefined();

    for (const end of ["pointercancel", "pointerleave"]) {
      await button.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
      await button.trigger(end);
      vi.advanceTimersByTime(1000);
    }
    expect(wrapper.emitted("hold")).toBeUndefined();
  });

  it("holds on a right click, without the browser's menu", async () => {
    const wrapper = tile();
    const menu = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
    wrapper.find("button").element.dispatchEvent(menu);
    expect(menu.defaultPrevented).toBe(true);
    expect(wrapper.emitted("hold")).toHaveLength(1);
  });

  it("holds on every right click", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    for (let time = 0; time < 2; time += 1) {
      await button.trigger("pointerdown", { button: 2, clientX: 10, clientY: 10 });
      await button.trigger("contextmenu");
      await button.trigger("pointerup", { button: 2 });
    }
    expect(wrapper.emitted("hold")).toHaveLength(2);
    await button.trigger("click");
    expect(wrapper.emitted("tap")).toHaveLength(1);
  });

  it("holds once when a long press also brings the context menu", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    await button.trigger("pointerdown", { button: 0, clientX: 10, clientY: 10 });
    vi.advanceTimersByTime(500);
    await button.trigger("contextmenu");
    await button.trigger("pointerup", { button: 0 });
    await button.trigger("click");
    expect(wrapper.emitted("hold")).toHaveLength(1);
    expect(wrapper.emitted("tap")).toBeUndefined();
  });

  it("holds with Shift+Enter from the keyboard", async () => {
    const wrapper = tile();
    const button = wrapper.find("button");
    const shifted = new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true });
    button.element.dispatchEvent(shifted);
    expect(shifted.defaultPrevented).toBe(true);
    expect(wrapper.emitted("hold")).toHaveLength(1);
    const plain = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    button.element.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(false);
    expect(wrapper.emitted("hold")).toHaveLength(1);
    expect(wrapper.emitted("tap")).toBeUndefined();
  });

  it("can be told how a screen reader should name it", () => {
    expect(tile({ label: "Chess. Touch and hold to invite" }).find("button").attributes("aria-label")).toBe(
      "Chess. Touch and hold to invite",
    );
  });

  it("breaks a name only between its words, never inside one", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).not.toMatch(/hyphens\s*:\s*auto/);
    expect(styles).not.toMatch(/anywhere|break-word|break-all/);
    const name = styles.match(/\.ft-app-tile__name\s*\{([^}]*)\}/)![1];
    expect(name).toMatch(/-webkit-line-clamp:\s*2/);
    expect(name).toMatch(/overflow-wrap:\s*normal/);
    expect(styles.match(/\.ft-app-tile__name,\s*\.ft-app-tile__probe\s*\{([^}]*)\}/)![1]).toMatch(/font-size:\s*12px/);
    expect(tile({ name: "Batalla naval" }).find(".ft-app-tile__name").text()).toBe("Batalla naval");
  });

  it("keeps the same height whatever the name, with room for two lines and the caption", () => {
    const styles = source.slice(source.indexOf("<style"));
    const rule = (selector: string) => styles.match(new RegExp(`${selector.replace(/[.]/g, "\\$&")}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
    expect(rule(".ft-app-tile__label")).toMatch(/min-block-size:/);
    expect(rule(".ft-app-tile__label--caption")).toMatch(/min-block-size:/);
    expect(tile().find(".ft-app-tile__label").classes()).not.toContain("ft-app-tile__label--caption");
    const offered = tile({ caption: "48 KB" }).find(".ft-app-tile__label");
    expect(offered.classes()).toContain("ft-app-tile__label--caption");
    expect(offered.find(".ft-app-tile__name").exists()).toBe(true);
    expect(offered.find(".ft-app-tile__caption").exists()).toBe(true);
  });

  describe("with a word wider than the tile", () => {
    let observers: Array<() => void>;

    beforeEach(() => {
      observers = [];
      vi.stubGlobal(
        "ResizeObserver",
        class {
          constructor(callback: () => void) {
            observers.push(callback);
          }
          observe() {}
          disconnect() {}
        },
      );
    });
    afterEach(() => {
      vi.unstubAllGlobals();
      document.head.querySelectorAll("style.base-size").forEach((css) => css.remove());
    });

    function laidOut(wrapper: ReturnType<typeof tile>, tileWidth: number, widestWordAt12: number) {
      const css = document.createElement("style");
      css.className = "base-size";
      css.textContent = ".ft-app-tile__probe { font-size: 12px; }";
      document.head.append(css);
      const probe = wrapper.find(".ft-app-tile__probe").element as HTMLElement;
      wrapper.find(".ft-app-tile__label").element.getBoundingClientRect = () => ({ width: tileWidth }) as DOMRect;
      probe.getBoundingClientRect = () => ({ width: (widestWordAt12 * parseFloat(getComputedStyle(probe).fontSize)) / 12 }) as DOMRect;
      for (const observer of observers) observer();
    }

    it("measures the widest word of the name without showing it or saying it twice", () => {
      const probe = tile({ name: "Rompecristales" }).find(".ft-app-tile__probe");
      expect(probe.attributes("data-name")).toBe("Rompecristales");
      expect(probe.attributes("aria-hidden")).toBe("true");
      expect(probe.text()).toBe("");
      const rules = source.slice(source.indexOf("<style")).matchAll(/\.ft-app-tile__probe\s*\{([^}]*)\}/g);
      const rule = [...rules].map((one) => one[1]).join("");
      expect(rule).toMatch(/position:\s*absolute/);
      expect(rule).toMatch(/visibility:\s*hidden/);
      expect(rule).toMatch(/width:\s*min-content/);
      expect(source).toMatch(/\.ft-app-tile__probe::before\s*\{[^}]*content:\s*attr\(data-name\)/);
    });

    it("shrinks that name just enough to fit, and keeps the whole name for screen readers", async () => {
      const wrapper = tile({ name: "Rompecristales" });
      laidOut(wrapper, 76, 87);
      await nextTick();
      const name = wrapper.find(".ft-app-tile__name");
      expect((name.element as HTMLElement).style.fontSize).toBe("10.4px");
      expect(name.classes()).not.toContain("ft-app-tile__name--clipped");
      expect(name.text()).toBe("Rompecristales");
      expect(wrapper.find("button").attributes("aria-label")).toBe("Rompecristales");
    });

    it("leaves a name whose words fit at the tile's usual size", async () => {
      const wrapper = tile({ name: "Batalla naval" });
      laidOut(wrapper, 76, 44);
      await nextTick();
      const name = wrapper.find(".ft-app-tile__name");
      expect((name.element as HTMLElement).style.fontSize).toBe("");
      expect(name.classes()).not.toContain("ft-app-tile__name--clipped");
    });

    it("ends the name with an ellipsis when even the smallest size is too wide", async () => {
      const wrapper = tile({ name: "Donaudampfschifffahrtsgesellschaft", badge: "lock" });
      laidOut(wrapper, 76, 240);
      await nextTick();
      const name = wrapper.find(".ft-app-tile__name");
      expect((name.element as HTMLElement).style.fontSize).toBe("9px");
      expect(name.classes()).toContain("ft-app-tile__name--clipped");
      expect(wrapper.find("button").attributes("aria-label")).toBe("Donaudampfschifffahrtsgesellschaft, locked");
      const rule = source.slice(source.indexOf("<style")).match(/\.ft-app-tile__name--clipped\s*\{([^}]*)\}/)![1];
      expect(rule).toMatch(/white-space:\s*nowrap/);
      expect(rule).toMatch(/text-overflow:\s*ellipsis/);
    });
  });

  describe("with the plugin's own image", () => {
    const IMAGE = "data:image/svg+xml;base64,PHN2Zy8+";

    it("draws the image in the icon box instead of the Ionicon, with the badge over it", () => {
      const wrapper = tile({ image: IMAGE, badge: "download", busy: true });
      const box = wrapper.find(".ft-app-tile__icon");
      expect(box.classes()).toContain("ft-app-tile__icon--image");
      const img = box.find("img");
      expect(img.attributes("src")).toBe(IMAGE);
      expect(img.attributes("alt")).toBe("");
      expect(img.attributes("draggable")).toBe("false");
      expect(icons(wrapper)).toEqual([downloadOutline]);
      expect(box.findComponent(IonSpinner).exists()).toBe(true);
      expect(tile().find("img").exists()).toBe(false);
    });

    it("fills the box, without the tint behind it, dimmed when not installed or busy", () => {
      const styles = source.slice(source.indexOf("<style"));
      const rule = (selector: string) => styles.match(new RegExp(`${selector.replace(/[.[\]"=]/g, "\\$&")}\\s*\\{([^}]*)\\}`))?.[1] ?? "";
      expect(rule(".ft-app-tile__image")).toMatch(/object-fit:\s*cover/);
      // Cut to the box's corners by its own radius: a clipped box would cut the badge off too.
      expect(rule(".ft-app-tile__image")).toMatch(/border-radius:\s*inherit/);
      expect(rule(".ft-app-tile .ft-app-tile__icon--image")).toMatch(/background:\s*transparent/);
      expect(rule(".ft-app-tile--off .ft-app-tile__image")).toMatch(/opacity:\s*0\.55/);
      expect(rule('.ft-app-tile[aria-busy="true"] .ft-app-tile__image')).toMatch(/opacity:/);
    });
  });
});
