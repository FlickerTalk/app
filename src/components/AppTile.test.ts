import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonIcon, IonSpinner } from "@ionic/vue";
import { addOutline, downloadOutline, imageOutline, lockClosedOutline } from "ionicons/icons";
import AppTile from "./AppTile.vue";

const tile = (props: Record<string, unknown> = {}) =>
  mount(AppTile, { props: { name: "Image", icon: imageOutline, ...props }, attachTo: document.body });

/** The icons drawn on the tile, in order: the app's own first, then its badge's. */
const icons = (wrapper: ReturnType<typeof tile>) => wrapper.findAllComponents(IonIcon).map((one) => one.props("icon"));

describe("AppTile", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    vi.useRealTimers();
    document.body.innerHTML = "";
  });

  // 2026-10-08 (plan of the apps grid): a tile is a home-screen icon with its name under it, a real
  // button with Ionic's ripple, named for a screen reader by the app's name.
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
    // The end-to-end tests look for the lock as they did on the rows before the grid.
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

  // Installing takes a moment: a spinner over the icon, in the icon's own box, so nothing moves.
  it("shows a spinner over its icon while busy, without growing", () => {
    const wrapper = tile({ busy: true, badge: "download" });
    const box = wrapper.find(".ft-app-tile__icon");
    expect(box.findComponent(IonSpinner).exists()).toBe(true);
    expect(box.findComponent(IonIcon).exists()).toBe(true);
    expect(wrapper.find("button").attributes("aria-busy")).toBe("true");
    expect(tile().findComponent(IonSpinner).exists()).toBe(false);
  });

  it("taps", async () => {
    const wrapper = tile();
    await wrapper.find("button").trigger("click");
    expect(wrapper.emitted("tap")).toHaveLength(1);
    expect(wrapper.emitted("hold")).toBeUndefined();
  });

  // Touch and hold, half a second, opens what the app is; the tap that ends it opens nothing.
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
    // The next tap is a tap again.
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

  // A finger that moves is scrolling the grid, not holding a tile.
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

  // Found end to end: a second right click held nothing, because no click had ended the first.
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

  // Android's long press also opens the context menu: one hold, not two.
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

  // Enter and Space are the button's own click; Shift+Enter holds, and does not tap.
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
});
