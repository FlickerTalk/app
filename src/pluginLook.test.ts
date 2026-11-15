import { afterEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";
import { followLook } from "./pluginLook";

const html = document.documentElement;
const fill = ref(false);
const tell = vi.fn();
const look = () => ({ dark: html.classList.contains("ft-dark"), fill: fill.value });
const Follower = defineComponent({
  setup() {
    followLook(look, tell, () => fill.value);
    return () => h("div");
  },
});

enableAutoUnmount(afterEach);

describe("followLook", () => {
  afterEach(() => {
    html.classList.remove("ft-dark");
    fill.value = false;
    tell.mockReset();
  });

  it("tells the plugin the new look when the page changes it", async () => {
    mount(Follower);
    html.classList.add("ft-dark");
    await flushPromises();
    expect(tell).toHaveBeenCalledWith({ type: "ft.theme", dark: true, fill: false });
  });

  it("tells the plugin when the watched state changes", async () => {
    mount(Follower);
    fill.value = true;
    await flushPromises();
    expect(tell).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, fill: true });
    fill.value = false;
    await flushPromises();
    expect(tell).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, fill: false });
    expect(tell).toHaveBeenCalledTimes(2);
  });

  it("says nothing when the look is what the plugin already has", async () => {
    mount(Follower);
    html.classList.add("ft-other");
    await flushPromises();
    html.classList.remove("ft-other");
    expect(tell).not.toHaveBeenCalled();
  });

  it("stops once its component is gone", async () => {
    const wrapper = mount(Follower);
    wrapper.unmount();
    html.classList.add("ft-dark");
    fill.value = true;
    await flushPromises();
    expect(tell).not.toHaveBeenCalled();
  });
});
