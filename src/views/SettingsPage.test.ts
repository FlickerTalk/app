import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonSelect, IonSelectOption, IonToggle } from "@ionic/vue";
import SettingsPage from "./SettingsPage.vue";
import { calls, seed } from "../__tests__/seed";
import { store } from "../core";

vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.3.1") }));
const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

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

  // §61: the encrypted backup comes after the MVP; nothing is shown before it works.
  it("offers moving the identity to a new phone, and nothing that does not work yet", () => {
    const text = mount(SettingsPage, { shallow: true }).text();
    expect(text).toContain("Move to a new phone");
    expect(text).not.toContain("Backup");
    expect(text).not.toContain("Privacy");
    expect(text).not.toContain("Export identity");
  });

  // §41: the free year, counted on this phone.
  it("shows how long the app stays free", () => {
    store.me.freeUntil = Date.now() + 100.5 * 24 * 3600 * 1000;
    expect(mount(SettingsPage, { shallow: true }).text()).toContain("Free · 100 days left");
  });

  it("shows the app's real version", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("0.3.1");
  });

  it("moves to a new phone from here, as the old phone", async () => {
    await mount(SettingsPage, { shallow: true }).find("[data-test='move']").trigger("click");
    expect(push).toHaveBeenCalledWith("/move?role=old");
  });

  it("lists the blocked contacts", async () => {
    await mount(SettingsPage, { shallow: true }).find("[data-test='blocked']").trigger("click");
    expect(push).toHaveBeenCalledWith("/blocked");
  });

  it("lets the user choose how calls are routed", () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findComponent(IonSelect);
    expect(select.attributes("aria-label")).toBe("Calls");
    expect(select.attributes("value")).toBe("auto");
    expect(wrapper.findAllComponents(IonSelectOption)).toHaveLength(3);
  });

  // PoC 0 is over (§87): no test screen in the app, not even in development builds.
  it("has no PoC screen", () => {
    expect(mount(SettingsPage, { shallow: true }).find("[data-test='poc']").exists()).toBe(false);
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

  // §78: erasing takes this device off our server and wipes the phone, so it asks first.
  it("erases the phone only after a confirmation", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const erase = wrapper.find("[data-test='erase']");
    expect(erase.exists()).toBe(true);
    await erase.trigger("click");
    expect(calls.map(([command]) => command)).not.toContain("core_erase");
    expect(wrapper.text()).toContain("Erase everything on this phone?");
    await wrapper.find("[data-test='erase-confirm']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_erase");
  });

  it("can change its mind about erasing", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='erase']").trigger("click");
    await wrapper.find("[data-test='erase-cancel']").trigger("click");
    expect(wrapper.find("[data-test='erase-confirm']").exists()).toBe(false);
    expect(calls.map(([command]) => command)).not.toContain("core_erase");
  });

  // Issue app#3: from here you see what runs inside FlickerTalk.
  it("opens the plugins screen", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='plugins']").trigger("click");
    expect(push).toHaveBeenCalledWith("/plugins");
  });

  // §40: the plan is a screen of its own, where the euro is asked for and the age is said.
  it("opens the plan", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='plan']").trigger("click");
    expect(push).toHaveBeenCalledWith("/plan");
  });

  // Hidden sessions: one row, no explanation, that goes to the PIN pad.
  it("goes to the PIN pad from a row", async () => {
    await mount(SettingsPage, { shallow: true }).find("[data-test='session']").trigger("click");
    expect(push).toHaveBeenCalledWith("/session");
  });

  // Issue app#6: the default for contacts added later; each contact can differ.
  it("turns receipts off for new contacts through the core", async () => {
    const toggle = mount(SettingsPage, { shallow: true })
      .findAllComponents(IonToggle)
      .find((one) => one.attributes("data-test") === "receipts")!;
    expect(toggle.attributes("checked")).toBe("true");
    toggle.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: false } }));
    await flushPromises();
    expect(calls).toContainEqual(["core_set_receipts", { enabled: false }]);
  });

  // Issue app#7: the weekly hours have their own page.
  it("goes to the weekly hours from a row", async () => {
    await mount(SettingsPage, { shallow: true }).find("[data-test='hours']").trigger("click");
    expect(push).toHaveBeenCalledWith("/hours");
  });
});
