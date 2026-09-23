import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonToggle } from "@ionic/vue";
import PluginsPage from "./PluginsPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: false, send: "nothing" },
  installedAt: 1_800_000_000_000,
};
const AI = {
  id: "com.flickertalk.ai",
  name: "Assistant",
  version: "0.2.0",
  asks: { network: ["api.openai.com"], messages: true, send: "propose" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1_800_000_000_000,
};

/** What the catalogue offers this phone; nothing travels inside the app (§56). */
const OFFERED = [
  { id: "com.flickertalk.code", name: "Code block", version: "1.0.0", summary: "Shows code.", size: 2048, installed: true },
  { id: "com.flickertalk.sketch", name: "Sketch", version: "1.0.0", summary: "Draw with a finger.", size: 3072, installed: false },
];

describe("PluginsPage", () => {
  beforeEach(() => {
    seed();
    // The seeded bridge is replaced, so the calls are recorded here.
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugins") return [CODE, AI];
      if (command === "core_catalogue") return OFFERED;
      return undefined;
    });
  });

  it("lists the plugins on this phone with their version", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Code block");
    expect(wrapper.text()).toContain("1.0.0");
    expect(wrapper.text()).toContain("Assistant");
  });

  // §53: what a plugin may do is shown one by one, and nothing is on until the user says so.
  it("shows every permission a plugin asks for, and whether it is on", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const text = wrapper.text();
    expect(text).toContain("Read what you send it");
    expect(text).toContain("api.openai.com");
    expect(text).toContain("Write in the chat");
    const toggles = wrapper.findAllComponents(IonToggle);
    expect(toggles.length).toBeGreaterThanOrEqual(4);
    expect(toggles[0].props("checked")).toBe(false);
  });

  it("grants a permission through the core", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    const toggle = wrapper.findAllComponents(IonToggle)[0];
    toggle.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    expect(calls).toContainEqual([
      "core_plugin_grant",
      { plugin: "com.flickertalk.code", granted: { network: [], messages: true, send: "nothing" } },
    ]);
  });

  it("removes a plugin after asking", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='remove-com.flickertalk.code']").trigger("click");
    expect(calls.map(([command]) => command)).not.toContain("core_plugin_remove");
    await wrapper.find("[data-test='remove-confirm']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_remove", { plugin: "com.flickertalk.code" }]);
  });

  // A tool that travels with the app is offered, never installed behind the user's back (§53).
  it("offers the tools the app carries that are not installed yet", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Sketch");
    expect(wrapper.text()).toContain("Draw with a finger.");
    expect(wrapper.find("[data-test='install-com.flickertalk.sketch']").exists()).toBe(true);
    expect(wrapper.find("[data-test='install-com.flickertalk.code']").exists()).toBe(false);
  });

  it("installs one when the user asks for it", async () => {
    const wrapper = mount(PluginsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='install-com.flickertalk.sketch']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_add", { plugin: "com.flickertalk.sketch" }]);
    expect(calls.filter(([command]) => command === "core_plugins").length).toBeGreaterThan(1);
  });
});
