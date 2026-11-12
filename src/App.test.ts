import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonRouterOutlet } from "@ionic/vue";
import App from "./App.vue";
import CallBar from "./components/CallBar.vue";
import { installTauri } from "./__tests__/tauri";
import { clearOnboarded, setOnboarded } from "./preferences";
import { premiumLocked } from "./plugins";

const push = vi.fn();
const router = { push, currentRoute: { value: { path: "/tabs/chats" } }, afterEach: () => () => undefined };
vi.mock("vue-router", async (importOriginal) => ({ ...(await importOriginal<typeof import("vue-router")>()), useRouter: () => router }));

describe("App", () => {
  beforeEach(() => {
    push.mockClear();
    installTauri();
  });

  it("opens the plugin of a tapped reminder", async () => {
    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r 1", session: null } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r%201");
  });

  it("opens the plugin of a tapped reminder in the session it was set in", async () => {
    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r1", session: "s 1" } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r1&session=s%201");
  });

  it("opens the plugin of a reminder tapped while the app was in the background", async () => {
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).not.toHaveBeenCalled();

    installTauri((command) => (command === "core_pending_reminder" ? { plugin: "com.flickertalk.notes", id: "r1", session: null } : undefined));
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/plugin/com.flickertalk.notes?reminder=r1");
  });

  it("knows whether the tools are locked, at start and back on the screen", async () => {
    let state = "trial";
    installTauri((command) => (command === "core_plan" ? { state, until: 0 } : undefined));
    mount(App, { shallow: true });
    await flushPromises();
    expect(premiumLocked.value).toBe(false);
    state = "limited";
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(premiumLocked.value).toBe(true);
    premiumLocked.value = false;
  });

  it("reads the installed plugins once at start", async () => {
    const asked: string[] = [];
    installTauri((command) => {
      asked.push(command);
      return command === "core_plugins" ? [] : undefined;
    });
    mount(App, { shallow: true });
    await flushPromises();
    expect(asked.filter((command) => command === "core_plugins")).toHaveLength(1);
  });

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

  it("opens Add contact when the phone opened the app with a contact link", async () => {
    setOnboarded();
    const asked: string[] = [];
    installTauri((command) => {
      asked.push(command);
      return command === "core_opened_link" ? { kind: "add", link: "https://flickertalk.com/add#card", valid: true } : undefined;
    });
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/add-contact");
    expect(asked).not.toContain("core_add_contact");
    clearOnboarded();
  });

  it("asks for an opened link again back on the screen, and listens for one", async () => {
    setOnboarded();
    let link: unknown = null;
    const listened: unknown[] = [];
    installTauri((command, args) => {
      if (command === "plugin:event|listen") listened.push(args?.event);
      return command === "core_opened_link" ? link : undefined;
    });
    mount(App, { shallow: true });
    await flushPromises();
    expect(push).not.toHaveBeenCalled();
    expect(listened).toContain("ft://opened-link");

    link = { kind: "move", link: "https://flickertalk.com/move#invite", valid: true };
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(push).toHaveBeenCalledWith("/move");
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

  it("hosts the bar that goes back to a call going on", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.findComponent(CallBar).exists()).toBe(true);
  });

  it("no longer shows the prototype design switcher", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.find("[aria-label='Design directions (prototype)']").exists()).toBe(false);
  });
});
