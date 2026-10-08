import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonIcon, IonModal, IonSelect, IonSelectOption, IonTextarea, IonToggle } from "@ionic/vue";
import { checkmarkOutline, copyOutline, sparklesOutline } from "ionicons/icons";
import SettingsPage from "./SettingsPage.vue";
import { calls, seed } from "../__tests__/seed";
import { PREMIUM_PAGE } from "../core";
import { installTauri } from "../__tests__/tauri";
import { store } from "../core";
import en from "../i18n/en.json";
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
// What the core tells the screen (`ft://…`); a test says it with `events.handlers.get(name)?.()`.
const events = vi.hoisted(() => ({ handlers: new Map<string, () => void>(), unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: () => void) => {
    events.handlers.set(name, handler);
    return events.unlisten;
  },
}));
const push = vi.fn();
// Where the app is: `/plan` and every locked premium thing land on Settings at `#premium`.
const route = vi.hoisted(() => ({ path: "/tabs/settings", hash: "" }));
vi.mock("vue-router", () => ({ useRouter: () => ({ push }), useRoute: () => route }));
// Ionic's toasts are overlays of the real app; here, what the screen asks of them.
const toast = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

const catalogues = import.meta.glob<{ plan: Record<string, unknown>; premium: Record<string, unknown> }>("../i18n/*.json", {
  eager: true,
  import: "default",
});

function texts(node: unknown): string[] {
  if (typeof node === "string") return [node];
  return typeof node === "object" && node !== null ? Object.values(node).flatMap(texts) : [];
}

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
  // Ioan, 2026-10-08 (mockup settings-premium.html): the Plan screen is gone. Settings has a
  // Premium section between the appearance and the move/backup groups: a note, the tools and the
  // sessions with a PIN, and, once the 15 days are over without paying, the subscription itself.
  describe("the Premium section", () => {
    const DAY = 24 * 60 * 60 * 1000;
    const section = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='premium']");
    const note = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='premium-note']");
    const tools = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='plugins']");
    const sessions = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='session']");
    const enter = (wrapper: ReturnType<typeof mount>) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillEnter ?? []).forEach((hook) => hook());

    /** What the core says about the plan, and what the Store says a year costs (null: it cannot say). */
    function planning(plan: Record<string, unknown>, price: string | null = "0,99 €") {
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_subscription_price") return { price };
        return command === "core_plan" ? plan : undefined;
      });
    }

    async function settings() {
      const wrapper = mount(SettingsPage, { shallow: true });
      await flushPromises();
      return wrapper;
    }

    beforeEach(() => {
      push.mockClear();
      route.hash = "";
      toast.create.mockReset().mockImplementation(async () => ({ present: toast.present }));
      toast.present.mockReset();
    });

    it("sits after the appearance group and before moving and the backup", async () => {
      planning({ state: "trial", until: Date.now() + 15 * DAY });
      const html = (await settings()).html();
      const at = (test: string) => html.indexOf(`data-test="${test}"`);
      expect(at("premium")).toBeGreaterThan(at("extra-tab"));
      expect(at("premium")).toBeLessThan(at("move"));
      expect(at("plugins")).toBeGreaterThan(at("premium"));
      expect(at("session")).toBeGreaterThan(at("premium"));
      expect(at("session")).toBeLessThan(at("move"));
    });

    it("is headed Premium, with the sparkles", async () => {
      planning({ state: "trial", until: Date.now() + 15 * DAY });
      const head = (await settings()).find("[data-test='premium-head']");
      expect(head.text()).toBe(en.premium.title);
      expect(head.findComponent(IonIcon).props("icon")).toBe(sparklesOutline);
    });

    it("has no Plan row any more, nor a lone plugins group", async () => {
      planning({ state: "trial", until: Date.now() + 15 * DAY });
      const wrapper = await settings();
      expect(wrapper.find("[data-test='plan']").exists()).toBe(false);
      expect(wrapper.findAll("[data-test='plugins']")).toHaveLength(1);
      expect(wrapper.findAll("[data-test='session']")).toHaveLength(1);
    });

    describe("in the 15 free days", () => {
      it("says what is free forever and what the trial is, with the Store's price", async () => {
        planning({ state: "trial", until: Date.now() + 15 * DAY - 5 });
        const wrapper = await settings();
        expect(note(wrapper).text()).toBe(en.premium.trialNote.replace("{price}", "0,99 €"));
      });

      it("counts the days left as every screen does, on both rows", async () => {
        planning({ state: "trial", until: Date.now() + 15 * DAY - 5 });
        const wrapper = await settings();
        const days = en.premium.daysLeft.replace("{days}", "15");
        expect(tools(wrapper).find("[data-test='premium-badge']").text()).toBe(days);
        expect(sessions(wrapper).find("[data-test='premium-badge']").text()).toBe(days);
        expect(tools(wrapper).find("[data-test='premium-badge']").attributes("color")).toBe("primary");
      });

      it("opens the tools and the PIN pad", async () => {
        planning({ state: "trial", until: Date.now() + 15 * DAY });
        const wrapper = await settings();
        expect(tools(wrapper).text()).toContain(en.premium.tools);
        expect(sessions(wrapper).text()).toContain(en.premium.sessions);
        await tools(wrapper).trigger("click");
        expect(push).toHaveBeenCalledWith("/plugins");
        await sessions(wrapper).trigger("click");
        expect(push).toHaveBeenCalledWith("/session");
      });

      it("asks for nothing", async () => {
        planning({ state: "trial", until: Date.now() + 15 * DAY });
        const wrapper = await settings();
        expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
        expect(wrapper.find("[data-test='restore']").exists()).toBe(false);
        expect(wrapper.find("[data-test='premium-locked']").exists()).toBe(false);
      });
    });

    describe("once the free days are over, without paying", () => {
      const limited = { state: "limited", until: 0 };

      it("says what the subscription is for, at the Store's price", async () => {
        planning(limited, "0,99 €");
        const wrapper = await settings();
        expect(note(wrapper).text()).toBe(en.premium.limitedNote.replace("{price}", "0,99 €"));
        expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribe.replace("{price}", "0,99 €"));
      });

      it("subscribes through the Store from the button", async () => {
        planning(limited);
        const wrapper = await settings();
        await wrapper.find("[data-test='pay']").trigger("click");
        await flushPromises();
        expect(calls.map(([command]) => command)).toContain("core_subscribe");
      });

      it("shows both rows locked, with no way in but the subscription", async () => {
        planning(limited);
        const wrapper = await settings();
        for (const [row, label] of [
          [tools(wrapper), en.plugins.locked],
          [sessions(wrapper), en.session.subscribeToUse],
        ] as const) {
          expect(row.classes()).toContain("ft-premium__locked");
          expect(row.attributes("detail")).toBe("false");
          expect(row.attributes("aria-label")).toBe(label);
          expect(row.find("[data-test='premium-locked']").exists()).toBe(true);
          expect(row.find("[data-test='premium-badge']").exists()).toBe(false);
          calls.length = 0;
          await row.trigger("click");
          await flushPromises();
          // §108: the lock stands in front of the PIN pad, never after a PIN.
          expect(push).not.toHaveBeenCalled();
          expect(calls.map(([command]) => command)).toContain("core_subscribe");
        }
      });

      it("offers to restore a purchase, and says what the Store found", async () => {
        installTauri((command, args) => {
          calls.push([command, args]);
          if (command === "core_restore_subscription") return "restored";
          return command === "core_plan" ? limited : undefined;
        });
        const wrapper = await settings();
        const restore = wrapper.find("[data-test='restore']");
        expect(restore.text()).toBe(en.plan.restore);
        calls.length = 0;
        await restore.trigger("click");
        await flushPromises();
        expect(calls.map(([command]) => command)).toContain("core_restore_subscription");
        expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: en.plan.restored }));
        expect(toast.present).toHaveBeenCalled();
        // And the plan is read again.
        expect(calls.map(([command]) => command)).toContain("core_plan");
      });

      it("says when the Store has nothing to restore", async () => {
        installTauri((command) => (command === "core_restore_subscription" ? "nothing" : command === "core_plan" ? limited : undefined));
        const wrapper = await settings();
        await wrapper.find("[data-test='restore']").trigger("click");
        await flushPromises();
        expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: en.plan.nothingToRestore }));
        expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
      });

      it("says a Store that does not answer a restore as it does when paying", async () => {
        installTauri((command) => {
          if (command === "core_restore_subscription") throw "store_unavailable";
          return command === "core_plan" ? limited : undefined;
        });
        const wrapper = await settings();
        await wrapper.find("[data-test='restore']").trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.store_unavailable);
        expect(toast.create).not.toHaveBeenCalled();
      });

      // The Store answers with a key, never a sentence; a key nobody wrote never reaches the screen.
      it("says in the user's words when the Store will not sell, and keeps unknown answers off", async () => {
        for (const [thrown, said] of [
          ["not_on_sale", en.plan.trouble.not_on_sale],
          ["BillingClient exploded at 0x7f", en.plan.trouble.failed],
        ]) {
          installTauri((command) => {
            if (command === "core_subscribe") throw thrown;
            return command === "core_plan" ? limited : undefined;
          });
          const wrapper = await settings();
          await wrapper.find("[data-test='pay']").trigger("click");
          await flushPromises();
          expect(wrapper.find("[data-test='trouble']").text()).toBe(said);
          expect(wrapper.text()).not.toContain("0x7f");
        }
      });

      it("says nothing when the user backs out of the Store", async () => {
        installTauri((command) => {
          if (command === "core_subscribe") throw "cancelled";
          return command === "core_plan" ? limited : undefined;
        });
        const wrapper = await settings();
        await wrapper.find("[data-test='pay']").trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
      });

      // Offline, on a desktop or with the product missing: no amount at all, and paying still works.
      it("names no amount when the Store cannot say the price", async () => {
        planning(limited, null);
        const wrapper = await settings();
        expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribeYearly);
        expect(note(wrapper).text()).toBe(en.premium.limitedNoteYearly);
        expect(section(wrapper).text()).not.toMatch(/€|\$|euro/i);
      });

      it("names no amount when asking the Store fails", async () => {
        installTauri((command) => {
          if (command === "core_subscription_price") throw "store_unavailable";
          return command === "core_plan" ? limited : undefined;
        });
        const wrapper = await settings();
        expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribeYearly);
      });
    });

    describe("subscribed", () => {
      it("says until when on iOS, where the Store gives a real expiry", async () => {
        const until = Date.parse("2027-09-23T10:00:00Z");
        planning({ state: "subscribed", until });
        const wrapper = await settings();
        expect(note(wrapper).text()).toBe(en.premium.paidUntil.replace("{until}", new Date(until).toLocaleDateString()));
      });

      // 2026-10-07: Google Play never tells the phone until when: no date that means nothing.
      it("says a Play subscription renews automatically, with no date", async () => {
        const until = Date.parse("2027-10-07T10:00:00Z");
        planning({ state: "subscribed", until, renews: true });
        const wrapper = await settings();
        expect(note(wrapper).text()).toBe(en.premium.paid);
        expect(wrapper.text()).not.toContain(new Date(until).toLocaleDateString());
      });

      it("shows both rows active, open, and asks for nothing", async () => {
        planning({ state: "subscribed", until: Date.now() + 300 * DAY });
        const wrapper = await settings();
        for (const row of [tools(wrapper), sessions(wrapper)]) {
          const badge = row.find("[data-test='premium-badge']");
          expect(badge.text()).toBe(en.premium.active);
          expect(badge.attributes("color")).toBe("success");
          expect(row.classes()).not.toContain("ft-premium__locked");
        }
        expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
        expect(wrapper.find("[data-test='restore']").exists()).toBe(false);
        await tools(wrapper).trigger("click");
        expect(push).toHaveBeenCalledWith("/plugins");
        await sessions(wrapper).trigger("click");
        expect(push).toHaveBeenCalledWith("/session");
      });
    });

    // Settings stays alive behind the tabs: back from the Store, the section says it without a restart.
    it("reads the plan and the price again every time the page is entered", async () => {
      let state = "limited";
      let price = "$0.99";
      installTauri((command) => {
        if (command === "core_subscription_price") return { price };
        return command === "core_plan" ? { state, until: state === "limited" ? 0 : Date.now() + 300 * DAY } : undefined;
      });
      const wrapper = await settings();
      expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribe.replace("{price}", "$0.99"));
      price = "0,99 €";
      enter(wrapper);
      await flushPromises();
      expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribe.replace("{price}", "0,99 €"));
      state = "subscribed";
      enter(wrapper);
      await flushPromises();
      expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
    });

    // Seen on an iPhone (2026-10-08): the App Store account changed and the old price stayed. It is
    // asked again when the app comes back; a Store that cannot answer leaves the last one known.
    it("asks the price again when the app comes back, and keeps the last one known", async () => {
      let price: string | null = "$0.99";
      installTauri((command) => {
        if (command === "core_subscription_price") return { price };
        return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
      });
      const wrapper = await settings();
      price = "US$0.99";
      document.dispatchEvent(new Event("visibilitychange"));
      await flushPromises();
      expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribe.replace("{price}", "US$0.99"));
      price = null;
      document.dispatchEvent(new Event("visibilitychange"));
      await flushPromises();
      expect(wrapper.find("[data-test='pay']").text()).toBe(en.premium.subscribe.replace("{price}", "US$0.99"));
    });

    // 2026-10-07: the Store changes its mind with the app open (a renewal, an expiry, an approved
    // Ask to Buy): the section follows, and what the Store said before is forgotten.
    it("follows the plan when the Store changes its mind, and forgets the old trouble", async () => {
      let state = "limited";
      installTauri((command) => {
        if (command === "core_subscribe") throw "pending_approval";
        return command === "core_plan" ? { state, until: state === "limited" ? 0 : Date.now() + 300 * DAY } : undefined;
      });
      const wrapper = await settings();
      await wrapper.find("[data-test='pay']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.pending_approval);
      state = "subscribed";
      events.handlers.get("ft://plan")?.();
      await flushPromises();
      expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
      expect(tools(wrapper).find("[data-test='premium-badge']").text()).toBe(en.premium.active);
      wrapper.unmount();
      expect(events.unlisten).toHaveBeenCalled();
    });

    // Ioan, 2026-10-08: no age rule; nothing in the section asks it or speaks of it.
    it("never speaks of an age", async () => {
      for (const state of ["trial", "limited", "subscribed"]) {
        planning({ state, until: Date.now() + 15 * DAY });
        const wrapper = await settings();
        expect(section(wrapper).text(), state).not.toMatch(/\b21\b|under|minor|birth/i);
        expect(wrapper.find("[data-test='young']").exists()).toBe(false);
      }
    });

    // `/plan` and every locked premium thing elsewhere lead here (the tools, the PIN pad).
    /** Ionic's content area, as the stub stands for it: where it scrolls to, and from where. */
    function scrollArea(wrapper: ReturnType<typeof mount>, from = 0) {
      const scrollToPoint = vi.fn().mockResolvedValue(undefined);
      const area = wrapper.find("ion-content-stub").element as HTMLElement & Record<string, unknown>;
      area.getScrollElement = async () => ({ scrollTop: from, getBoundingClientRect: () => ({ top: 100 }) });
      area.scrollToPoint = scrollToPoint;
      return scrollToPoint;
    }
    const entered = async (wrapper: ReturnType<typeof mount>) => {
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewDidEnter ?? []).forEach((hook) => hook());
      await flushPromises();
    };

    it("brings the section to the top when the app asks for it", async () => {
      planning({ state: "limited", until: 0 });
      route.hash = "#premium";
      const wrapper = await settings();
      const scrolled = scrollArea(wrapper, 40);
      section(wrapper).element.getBoundingClientRect = () => ({ top: 700 }) as DOMRect;
      await entered(wrapper);
      expect(scrolled).toHaveBeenCalledWith(0, 640, expect.any(Number));
      expect(PREMIUM_PAGE).toBe("/tabs/settings#premium");
    });

    it("stays where it is when nothing asks for the section", async () => {
      planning({ state: "trial", until: Date.now() + 15 * DAY });
      const wrapper = await settings();
      const scrolled = scrollArea(wrapper);
      await entered(wrapper);
      expect(scrolled).not.toHaveBeenCalled();
    });
  });

  // No language may carry a price of its own: an amount in a catalogue would be wrong in every other
  // store, and the day the price changes. Nor an age rule (2026-10-08).
  it("has no amount of money nor an age in the Premium and Plan texts of any language", () => {
    const money = /[€$£¥₹₩₽]|\beuros?\b|ユーロ|유로|欧元|歐元|ยูโร|यूरो|ইউরো|євро|евро|يورو/i;
    for (const [path, catalogue] of Object.entries(catalogues)) {
      for (const text of [...texts(catalogue.plan), ...texts(catalogue.premium)]) {
        expect(text, path).not.toMatch(money);
        expect(text, path).not.toMatch(/\b21\b/);
      }
      for (const key of ["young", "iAmYoung", "iAmOlder"]) expect(catalogue.plan, path).not.toHaveProperty(key);
      for (const key of ["subscribe", "trialNote", "limitedNote"]) expect(String(catalogue.premium[key]), `${path} ${key}`).toContain("{price}");
    }
  });
});
