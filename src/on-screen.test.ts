import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import { useOnScreen } from "./on-screen";

// A page using it, as Ionic's outlet sees it: the hooks it registered, by name.
type ViewHook = "onIonViewWillEnter" | "onIonViewDidEnter" | "onIonViewWillLeave";
function page(entered?: () => void) {
  const Page = defineComponent({
    setup() {
      const onScreen = useOnScreen(entered);
      return () => h("div", { "data-on-screen": String(onScreen.value) });
    },
  });
  const wrapper = mount(Page);
  const fire = (hook: ViewHook) =>
    ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[hook] ?? []).forEach((one) => one());
  const onScreen = async () => {
    await wrapper.vm.$nextTick();
    return wrapper.attributes("data-on-screen") === "true";
  };
  return { fire, onScreen };
}

// app#179 and app#193: Ionic keeps a page mounted under the next one. It says the page leaves as
// soon as the next one starts to come in, but that it came in only once its own transition is over.
describe("useOnScreen", () => {
  it("starts on screen", async () => {
    const view = page();
    expect(await view.onScreen()).toBe(true);
  });

  it("is off screen once the page starts to leave", async () => {
    const view = page();
    view.fire("onIonViewWillLeave");
    expect(await view.onScreen()).toBe(false);
  });

  it("is back on screen once the page has come in again", async () => {
    const view = page();
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(true);
  });

  it("stays off screen when a page opened over it while it still came in", async () => {
    const view = page();
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(false);
  });

  it("is on screen again the next time the page comes in", async () => {
    const view = page();
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewDidEnter");
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(true);
  });

  it("ignores a \"did enter\" with no \"will enter\" before it", async () => {
    const view = page();
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(false);
  });

  it("tells the page each time a \"did enter\" counts, and only then", () => {
    const entered = vi.fn();
    const view = page(entered);
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewDidEnter");
    expect(entered).toHaveBeenCalledTimes(1);
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewWillLeave");
    view.fire("onIonViewDidEnter");
    expect(entered).toHaveBeenCalledTimes(1);
    view.fire("onIonViewWillEnter");
    view.fire("onIonViewDidEnter");
    expect(entered).toHaveBeenCalledTimes(2);
  });
});

// app#197: a tab page sits inside the tabs page. When a page is pushed over the tabs, Ionic tells
// only the tabs page it leaves, never the tab inside it.
describe("useOnScreen in a page inside another page", () => {
  function nested() {
    const Inner = defineComponent({
      setup() {
        const onScreen = useOnScreen();
        return () => h("i", { "data-on-screen": String(onScreen.value) });
      },
    });
    const Outer = defineComponent({
      setup() {
        useOnScreen();
        return () => h(Inner);
      },
    });
    const wrapper = mount(Outer);
    const fire = (vm: unknown, hook: ViewHook) =>
      ((vm as Record<string, Array<() => void> | undefined>)[hook] ?? []).forEach((one) => one());
    return {
      outer: (hook: ViewHook) => fire(wrapper.vm, hook),
      inner: (hook: ViewHook) => fire(wrapper.findComponent(Inner).vm, hook),
      onScreen: async () => {
        await wrapper.vm.$nextTick();
        return wrapper.find("i").attributes("data-on-screen") === "true";
      },
    };
  }

  it("is off screen while the page holding it is covered", async () => {
    const view = nested();
    expect(await view.onScreen()).toBe(true);
    view.outer("onIonViewWillLeave");
    expect(await view.onScreen()).toBe(false);
  });

  it("is back on screen once the page holding it has come in again", async () => {
    const view = nested();
    view.outer("onIonViewWillLeave");
    view.outer("onIonViewWillEnter");
    view.outer("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(true);
  });

  it("stays off screen when a page opened over the one holding it while that one still came in", async () => {
    const view = nested();
    view.outer("onIonViewWillLeave");
    view.outer("onIonViewWillEnter");
    view.outer("onIonViewWillLeave");
    view.outer("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(false);
  });

  it("stays off screen when the page holding it comes back showing another of its pages", async () => {
    const view = nested();
    // Another tab, then a page over the tabs, then back to the tabs: still the other tab.
    view.inner("onIonViewWillLeave");
    view.outer("onIonViewWillLeave");
    view.outer("onIonViewWillEnter");
    view.outer("onIonViewDidEnter");
    expect(await view.onScreen()).toBe(false);
  });
});
