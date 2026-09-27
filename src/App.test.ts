import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonRouterOutlet } from "@ionic/vue";
import App from "./App.vue";
import { installTauri } from "./__tests__/tauri";

const push = vi.fn();
vi.mock("vue-router", async (importOriginal) => ({ ...(await importOriginal<typeof import("vue-router")>()), useRouter: () => ({ push }) }));

describe("App", () => {
  beforeEach(() => {
    push.mockClear();
    installTauri();
  });

  // 2026-09-27: a reminder notification opens the app straight on the plugin that set it.
  it("opens the plugin of a tapped reminder", async () => {
    installTauri((command) => (command === "core_pending_reminder" ? "com.flickertalk.notes\nr 1" : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r%201");
  });

  it("goes nowhere special when no reminder was tapped", async () => {
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).not.toHaveBeenCalled();
  });

  it("hosts the router outlet", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.findComponent(IonRouterOutlet).exists()).toBe(true);
  });

  it("no longer shows the prototype design switcher", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.find("[aria-label='Design directions (prototype)']").exists()).toBe(false);
  });
});
