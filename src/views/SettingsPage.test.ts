import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonIcon, IonModal, IonSelect, IonSelectOption, IonTextarea, IonToggle } from "@ionic/vue";
import { checkmarkOutline, copyOutline } from "ionicons/icons";
import SettingsPage from "./SettingsPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { store } from "../core";
import { extraTab, storedExtraTab } from "../preferences";

// Android's back button: the handler the app listens with while something is open on top.
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => Promise.resolve("0.3.1"),
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => void (back.handler === handler && (back.handler = null)) };
  },
}));
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
  it("offers moving the identity to a new phone and the backup, and nothing that does not work yet", () => {
    const text = mount(SettingsPage, { shallow: true }).text();
    expect(text).toContain("Move to a new phone");
    // 2026-09-27: a sealed copy of the phone in the user's own cloud (§61).
    expect(text).toContain("Backup");
    expect(text).not.toContain("Privacy");
    expect(text).not.toContain("Export identity");
  });

  // §41: the free year, counted on this phone. Settings and the Plan screen count it the same
  // way (`daysLeft`): a phone installed a moment ago reads 365 days on both, not 364 here.
  it("shows how long the app stays free", () => {
    store.me.freeUntil = Date.now() + 365 * 24 * 3600 * 1000 - 5;
    expect(mount(SettingsPage, { shallow: true }).text()).toContain("Free · 365 days left");
  });

  it("says the free year is over once it is", () => {
    store.me.freeUntil = Date.now() - 1000;
    expect(mount(SettingsPage, { shallow: true }).text()).toContain("Free year over");
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

  // Seen in the UI review (2026-10-02): the two buttons beside the ID did nothing.
  describe("beside the ID", () => {
    const copy = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='copy-id']");

    it("copies the ID shown, says so for a moment, and is the copy button again after", async () => {
      vi.useFakeTimers();
      const writeText = vi.fn().mockResolvedValue(undefined);
      Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
      const wrapper = mount(SettingsPage, { shallow: true });
      expect(copy(wrapper).attributes("aria-label")).toBe("Copy ID");
      await copy(wrapper).trigger("click");
      await flushPromises();
      expect(writeText).toHaveBeenCalledWith(store.me.id);
      expect(copy(wrapper).attributes("aria-label")).toBe("ID copied");
      expect(copy(wrapper).findComponent(IonIcon).props("icon")).toBe(checkmarkOutline);
      vi.advanceTimersByTime(2000);
      await flushPromises();
      expect(copy(wrapper).attributes("aria-label")).toBe("Copy ID");
      expect(copy(wrapper).findComponent(IonIcon).props("icon")).toBe(copyOutline);
      vi.useRealTimers();
    });

    // §84: honest; a phone that did not take it to the clipboard is not told it did.
    it("says nothing was copied when the phone refuses", async () => {
      const writeText = vi.fn().mockRejectedValue(new Error("not allowed"));
      Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
      const wrapper = mount(SettingsPage, { shallow: true });
      await copy(wrapper).trigger("click");
      await flushPromises();
      expect(writeText).toHaveBeenCalled();
      expect(copy(wrapper).attributes("aria-label")).toBe("Copy ID");
    });

    it("shows my QR code, the one that adds me", async () => {
      push.mockClear();
      await mount(SettingsPage, { shallow: true }).find("[data-test='show-qr']").trigger("click");
      expect(push).toHaveBeenCalledWith("/add-contact");
    });
  });

  it("lists the blocked contacts", async () => {
    await mount(SettingsPage, { shallow: true }).find("[data-test='blocked']").trigger("click");
    expect(push).toHaveBeenCalledWith("/blocked");
  });

  // 2026-10-02: an anonymous suggestion. Ioan: an Ionic modal over Settings, not a page of its own.
  it("opens the suggestion modal over Settings, without leaving them", async () => {
    push.mockClear();
    const wrapper = mount(SettingsPage, { shallow: true, global: { stubs: { FeedbackModal: false } } });
    const modal = () => wrapper.findComponent(IonModal);
    expect(modal().props("isOpen")).toBe(false);
    const entry = wrapper.find("[data-test='feedback']");
    expect(entry.text()).toContain("Suggest something");
    await entry.trigger("click");
    expect(modal().props("isOpen")).toBe(true);
    expect(push).not.toHaveBeenCalled();
  });

  // Android's Back closes the modal, as it closes the app's other overlays, and stays in Settings.
  it("closes the suggestion modal with the back button", async () => {
    push.mockClear();
    const wrapper = mount(SettingsPage, { shallow: true, global: { stubs: { FeedbackModal: false } } });
    await wrapper.find("[data-test='feedback']").trigger("click");
    await flushPromises();
    back.handler?.();
    await flushPromises();
    expect(wrapper.findComponent(IonModal).props("isOpen")).toBe(false);
    expect(push).not.toHaveBeenCalled();
  });

  // Nothing of a suggestion is kept: closing forgets what was written and what came of it.
  it("opens the suggestion modal empty again after closing it", async () => {
    installTauri((command) => (command === "core_send_feedback" ? "failed" : undefined));
    const wrapper = mount(SettingsPage, { shallow: true, global: { stubs: { FeedbackModal: false } } });
    const modal = () => wrapper.findComponent(IonModal);
    await wrapper.find("[data-test='feedback']").trigger("click");
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "Stickers");
    await flushPromises();
    await wrapper.find("[data-test='send']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='outcome']").exists()).toBe(true);
    await wrapper.find("[data-test='feedback-close']").trigger("click");
    expect(modal().props("isOpen")).toBe(false);
    // Ionic says when the modal has gone.
    modal().vm.$emit("didDismiss", new CustomEvent("didDismiss"));
    await flushPromises();

    await wrapper.find("[data-test='feedback']").trigger("click");
    expect(modal().props("isOpen")).toBe(true);
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
    expect(wrapper.find("[data-test='outcome']").exists()).toBe(false);
  });

  it("lets the user choose how calls are routed", () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findAllComponents(IonSelect).find((one) => one.attributes("aria-label") === "Calls");
    expect(select?.attributes("value")).toBe("auto");
    expect(select?.findAllComponents(IonSelectOption)).toHaveLength(3);
  });

  // The core keeps a copy: a call answered from CallKit has no WebView to ask (2026-09-28).
  it("tells the core when the call routing changes", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findAllComponents(IonSelect).find((one) => one.attributes("aria-label") === "Calls");
    select?.vm.$emit("ionChange", { detail: { value: "always" } });
    await flushPromises();
    expect(calls).toContainEqual(["core_set_call_routing", { routing: "always" }]);
  });

  // A4: up to what size a file comes on its own is this phone's choice.
  it("lets the user choose up to what size files download on their own", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findAllComponents(IonSelect).find((one) => one.attributes("aria-label") === "Auto-download files");
    // A limit saved as 10 × 1024² bytes (the core's old default) shows as the round choice (app#74).
    expect(select?.attributes("value")).toBe(String(10_000_000));
    expect(select?.findAllComponents(IonSelectOption).map((option) => option.text())).toEqual([
      "Always ask",
      // A no-break space keeps the number with its unit when the value wraps ("Bis 10 / MB").
      "Up to 10\u00a0MB",
      "Up to 100\u00a0MB",
      "Up to 1\u00a0GB",
      "Always",
    ]);
    select?.vm.$emit("ionChange", { detail: { value: 0 } });
    await flushPromises();
    expect(calls).toContainEqual(["core_set_auto_download", { bytes: 0 }]);
  });

  // A5: renewing the link cuts off whoever kept the old one, so it asks once.
  it("renews the link after asking once", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='renew-link']").trigger("click");
    expect(calls.some(([command]) => command === "core_renew_link")).toBe(false);
    await wrapper.find("[data-test='renew-confirm']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_renew_link", {}]);
    expect(wrapper.text()).toContain("Link renewed");
  });

  // PoC 0 is over (§87): no test screen in the app, not even in development builds.
  it("has no PoC screen", () => {
    expect(mount(SettingsPage, { shallow: true }).find("[data-test='poc']").exists()).toBe(false);
  });

  it("lets the user pick one of the four colors", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    expect(wrapper.findAll(".ft-swatch").map((swatch) => swatch.attributes("aria-label"))).toEqual(["Mono", "Ember", "Aurora", "Lime"]);
    await wrapper.find("button[aria-label='Aurora']").trigger("click");
    expect(document.documentElement.dataset.direction).toBe("aurora");
    expect(wrapper.find("button[aria-label='Aurora']").attributes("aria-pressed")).toBe("true");
  });

  it("applies the lime colors when they are picked", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("button[aria-label='Lime']").trigger("click");
    expect(document.documentElement.dataset.direction).toBe("lime");
    expect(localStorage.getItem("ft-direction")).toBe("lime");
    expect(wrapper.find("button[aria-label='Lime']").attributes("aria-pressed")).toBe("true");
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

  // 2026-09-30: on the iPhone the app starts again in place, after a moment; meanwhile the screen
  // says the phone is being erased instead of looking as if nothing happened.
  it("says the phone is being erased once it is confirmed", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='erase']").trigger("click");
    await wrapper.find("[data-test='erase-confirm']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='erasing']").text()).toContain("Erasing this phone");
    expect(wrapper.find("[data-test='erase']").exists()).toBe(false);
    expect(wrapper.find("[data-test='erase-confirm']").exists()).toBe(false);
  });

  it("says so when the phone could not be erased, and lets the user try again", async () => {
    installTauri((command) => {
      if (command === "core_erase") throw new Error("disk");
      return undefined;
    });
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='erase']").trigger("click");
    await wrapper.find("[data-test='erase-confirm']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='erasing']").exists()).toBe(false);
    expect(wrapper.text()).toContain("This phone could not be erased");
    expect(wrapper.find("[data-test='erase']").exists()).toBe(true);
  });

  it("can change its mind about erasing", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='erase']").trigger("click");
    await wrapper.find("[data-test='erase-cancel']").trigger("click");
    expect(wrapper.find("[data-test='erase-confirm']").exists()).toBe(false);
    expect(calls.map(([command]) => command)).not.toContain("core_erase");
  });

  // 2026-10-05 (Ioan): the fourth tab is picked here, in a sheet: nothing, the games or the plugins.
  it("lets the user choose what the tab bar shows besides Chats, Calls and Settings", () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findAllComponents(IonSelect).find((one) => one.attributes("aria-label") === "Show in the tab bar");
    expect(select?.attributes("value")).toBe("none");
    expect(select?.attributes("interface")).toBe("modal");
    expect(select?.findAllComponents(IonSelectOption).map((option) => option.text())).toEqual(["Nothing", "Games", "Plugins"]);
    expect(select?.findAllComponents(IonSelectOption).map((option) => option.attributes("value"))).toEqual(["none", "games", "plugins"]);
  });

  it("keeps the chosen tab on this phone, and the bar follows at once", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    const select = wrapper.findAllComponents(IonSelect).find((one) => one.attributes("aria-label") === "Show in the tab bar");
    select?.vm.$emit("ionChange", { detail: { value: "games" } });
    await flushPromises();
    expect(storedExtraTab()).toBe("games");
    expect(extraTab.value).toBe("games");
    expect(select?.attributes("value")).toBe("games");
    select?.vm.$emit("ionChange", { detail: { value: "none" } });
    await flushPromises();
    expect(storedExtraTab()).toBe("none");
  });

  // Issue app#3: from here you see what runs inside FlickerTalk.
  it("opens the plugins screen", async () => {
    const wrapper = mount(SettingsPage, { shallow: true });
    await wrapper.find("[data-test='plugins']").trigger("click");
    expect(push).toHaveBeenCalledWith("/plugins");
  });

  // §40: the plan is a screen of its own, where the subscription is asked for and the age is said.
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
