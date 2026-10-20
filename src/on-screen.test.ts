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
