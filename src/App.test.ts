import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonRouterOutlet } from "@ionic/vue";
import App from "./App.vue";
import CallBar from "./components/CallBar.vue";
import { installTauri } from "./__tests__/tauri";
import { clearOnboarded, setOnboarded } from "./preferences";
import { toolsLocked } from "./plugins";

const push = vi.fn();
vi.mock("vue-router", async (importOriginal) => ({ ...(await importOriginal<typeof import("vue-router")>()), useRouter: () => ({ push }) }));

describe("App", () => {
  beforeEach(() => {
    push.mockClear();
    installTauri();
  });

  // 2026-09-27: a reminder notification opens the app straight on the plugin that set it.
  it("opens the plugin of a tapped reminder", async () => {
    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r 1", session: null } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r%201");
  });

  // 2026-10-01 (§108): one set inside a hidden session opens the plugin in that session.
  it("opens the plugin of a tapped reminder in the session it was set in", async () => {
    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r1", session: "s 1" } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r1&session=s%201");
  });

  // Found on a real phone (2026-09-27): with the app in the background, tapping the reminder
  // brings it back without mounting it again, so the app asks once more when it is visible.
  it("opens the plugin of a reminder tapped while the app was in the background", async () => {
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).not.toHaveBeenCalled();

    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r1", session: null } : undefined));
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r1");
  });

  // Ioan, 2026-10-08: the tools' lock follows the plan from the start, and again when the app comes
  // back to the screen (the free year may have ended while it was away).
  it("knows whether the tools are locked, at start and back on the screen", async () => {
    let state = "trial";
    installTauri((command) => (command === "core_plan" ? { state, until: 0 } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(toolsLocked.value).toBe(false);
    state = "limited";
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(toolsLocked.value).toBe(true);
    toolsLocked.value = false;
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
