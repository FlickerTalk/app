import { afterEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";
import { focusButton, useFullscreenCues } from "./gameFullscreen";

const on = ref(false);
const quiet = ref(false);
const inside = { focus: vi.fn() };
const outside = { focus: vi.fn() };
let said = ref("");
const Cued = defineComponent({
  setup() {
    ({ said } = useFullscreenCues({ on: () => on.value, inside: () => inside, outside: () => outside as unknown as HTMLElement, quiet: () => quiet.value }));
    return () => h("div");
  },
});

enableAutoUnmount(afterEach);

describe("useFullscreenCues", () => {
  afterEach(() => {
    on.value = false;
    quiet.value = false;
    inside.focus.mockReset();
    outside.focus.mockReset();
  });

  it("puts the focus inside and says so when full screen starts", async () => {
    mount(Cued);
    on.value = true;
    await flushPromises();
    expect(inside.focus).toHaveBeenCalledTimes(1);
    expect(outside.focus).not.toHaveBeenCalled();
    expect(said.value).toBe("Full screen");
  });

  it("puts the focus back outside and says so when it ends", async () => {
    mount(Cued);
    on.value = true;
    await flushPromises();
    on.value = false;
    await flushPromises();
    expect(outside.focus).toHaveBeenCalledTimes(1);
    expect(said.value).toBe("Exit full screen");
  });

  it("does neither when it ends while something else takes over", async () => {
    mount(Cued);
    on.value = true;
    await flushPromises();
    said.value = "";
    quiet.value = true;
    on.value = false;
    await flushPromises();
    expect(outside.focus).not.toHaveBeenCalled();
    expect(said.value).toBe("");
  });
});

describe("focusButton", () => {
  it("focuses the native button inside an Ionic button's shadow root once Ionic has drawn it", async () => {
    const host = document.createElement("ion-fake-button");
    const inner = document.createElement("button");
    Object.assign(host, {
      componentOnReady: async () => {
        host.attachShadow({ mode: "open" }).append(inner);
        return host;
      },
    });
    const focus = vi.spyOn(inner, "focus");
    await focusButton(host);
    expect(focus).toHaveBeenCalled();
  });

  it("focuses the element itself when it has no shadow root", async () => {
    const plain = document.createElement("button");
    const focus = vi.spyOn(plain, "focus");
    await focusButton(plain);
    expect(focus).toHaveBeenCalled();
    await expect(focusButton(undefined)).resolves.toBeUndefined();
  });
});
