import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonRouterOutlet } from "@ionic/vue";
import App from "./App.vue";
import CallBar from "./components/CallBar.vue";
import { installTauri } from "./__tests__/tauri";
import { clearOnboarded, setOnboarded } from "./preferences";

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

  // Found on a real phone (2026-09-27): with the app in the background, tapping the reminder
  // brings it back without mounting it again, so the app asks once more when it is visible.
  it("opens the plugin of a reminder tapped while the app was in the background", async () => {
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).not.toHaveBeenCalled();

    installTauri((command) => (command === "core_pending_reminder" ? "com.flickertalk.notes\nr1" : undefined));
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r1");
  });

  // iOS cuts the socket of a suspended app (2026-09-28): back on the screen, or opened from a
  // push, the app reconnects at once and so fetches what waits, instead of waiting for it.
  it("reconnects to the router at once when it comes back to the screen", async () => {
    const asked: string[] = [];
    installTauri((command) => {
      asked.push(command);
      return undefined;
    });
    mount(App, { shallow: true });
    await flushPromises();
    asked.length = 0;
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(asked).toContain("core_resume");
  });

  // Found on the iPhone (2026-09-28): a push registration that failed (the router was being
  // deployed) left the phone unreachable until the app was started again. Back on the screen it
  // hands its push token over once more, but never before the welcome asked for permission.
  it("hands the push token over again when it comes back to the screen", async () => {
    const asked: string[] = [];
    installTauri((command) => {
      asked.push(command);
      return undefined;
    });
    mount(App, { shallow: true });
    await flushPromises();

    clearOnboarded();
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(asked).not.toContain("core_enable_push");

    setOnboarded();
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(asked).toContain("core_enable_push");
    clearOnboarded();
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

  // 2026-09-29: a call going on can be gone back to from any screen.
  it("hosts the bar that goes back to a call going on", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.findComponent(CallBar).exists()).toBe(true);
  });

  it("no longer shows the prototype design switcher", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.find("[aria-label='Design directions (prototype)']").exists()).toBe(false);
  });
});
