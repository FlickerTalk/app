import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonSelect, IonSelectOption, IonToggle } from "@ionic/vue";
import SettingsPage from "./SettingsPage.vue";
import { calls, seed } from "../__tests__/seed";

describe("SettingsPage", () => {
  beforeEach(() => {
    localStorage.clear();
    seed();
  });

  it("shows the FlickerTalk ID of this device", () => {
    expect(mount(SettingsPage, { shallow: true }).text()).toContain("ft_74MxNcJ8E2kQ");
  });

  it("has the offline mailbox enabled by default", () => {
    const toggle = mount(SettingsPage, { shallow: true }).findComponent(IonToggle);
    expect(toggle.attributes("aria-label")).toBe("Offline mailbox");
    expect(toggle.attributes("checked")).toBe("true");
  });

  // Plan §19: the preference lives in the core and travels to contacts, never to the server.
  it("turns the mailbox off through the core", async () => {
    const toggle = mount(SettingsPage, { shallow: true }).findComponent(IonToggle);
    toggle.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: false } }));
    await flushPromises();
    expect(calls).toContainEqual(["core_set_mailbox", { enabled: false }]);
  });

  it("offers moving the identity to a new phone and an encrypted backup", () => {
    const text = mount(SettingsPage, { shallow: true }).text();
    expect(text).toContain("Move to a new phone");
    expect(text).toContain("Backup");
    expect(text).not.toContain("Export identity");
  });

  it("lets the user choose how calls are routed", () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findComponent(IonSelect);
    expect(select.attributes("aria-label")).toBe("Calls");
    expect(select.attributes("value")).toBe("auto");
    expect(wrapper.findAllComponents(IonSelectOption)).toHaveLength(3);
  });

  it("links to the PoC screen in development builds", () => {
    expect(mount(SettingsPage, { shallow: true }).find("[data-test='poc']").exists()).toBe(true);
  });

  it("lets the user pick one of the three colors", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    for (const label of ["Ember", "Aurora", "Mono"]) {
      expect(wrapper.find(`button[aria-label='${label}']`).exists()).toBe(true);
    }
    await wrapper.find("button[aria-label='Aurora']").trigger("click");
    expect(document.documentElement.dataset.direction).toBe("aurora");
    expect(wrapper.find("button[aria-label='Aurora']").attributes("aria-pressed")).toBe("true");
  });

  it("lets the user choose a light, dark or system appearance", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("button[aria-label='Light']").trigger("click");
    expect(document.documentElement.classList.contains("ft-dark")).toBe(false);
    await wrapper.find("button[aria-label='Dark']").trigger("click");
    expect(document.documentElement.classList.contains("ft-dark")).toBe(true);
    expect(wrapper.find("button[aria-label='System']").exists()).toBe(true);
  });
});
