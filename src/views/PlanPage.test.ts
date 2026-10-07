import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import PlanPage from "./PlanPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import en from "../i18n/en.json";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

// What the core tells the screen (`ft://…`); a test says it with `events.handlers.get(name)?.()`.
const events = vi.hoisted(() => ({ handlers: new Map<string, () => void>(), unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: async (name: string, handler: () => void) => {
    events.handlers.set(name, handler);
    return events.unlisten;
  },
}));

// Ionic's toasts are overlays of the real app; here, what the screen asks of them.
const toast = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

const DAY = 24 * 60 * 60 * 1000;

/**
 * What the core says about this phone's plan (§40–§47), and what the Store says a year costs
 * (`price`, formatted by the Store; `null` when it cannot say).
 */
function planning(plan: Record<string, unknown>, price: string | null = "0,99 €") {
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_subscription_price") return { price };
    return command === "core_plan" ? plan : undefined;
  });
}

const catalogues = import.meta.glob<{ plan: Record<string, unknown> }>("../i18n/*.json", {
  eager: true,
  import: "default",
});

function texts(node: unknown): string[] {
  if (typeof node === "string") return [node];
  return typeof node === "object" && node !== null ? Object.values(node).flatMap(texts) : [];
}

describe("PlanPage", () => {
  beforeEach(() => {
    seed();
    toast.create.mockReset().mockImplementation(async () => ({ present: toast.present }));
    toast.present.mockReset();
  });

  it("says how long the free year has left", async () => {
    planning({ state: "trial", until: Date.now() + 40 * DAY });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("40");
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
  });

  // The same free days read the same on Settings (`daysLeft`): installed a moment ago, 15 days.
  it("counts the free year as Settings does", async () => {
    planning({ state: "trial", until: Date.now() + 15 * DAY - 5 });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='where']").text()).toBe(en.plan.trial.replace("{days}", "15"));
  });

  // Ioan, 2026-10-08: there is no age rule any more. Nothing on the screen asks it or speaks of it.
  it("never asks the age", async () => {
    planning({ state: "limited", until: 0 });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='young']").exists()).toBe(false);
    expect(wrapper.find("[data-test='older']").exists()).toBe(false);
    expect(wrapper.text()).not.toMatch(/21|under|minor|birth/i);
    expect(calls.map(([command]) => command)).not.toContain("core_set_age");
  });

  // What is free forever and what the subscription is for, said in every state.
  it("says what is free forever and that the subscription is for the tools", async () => {
    for (const state of ["trial", "limited", "subscribed"]) {
      planning({ state, until: Date.now() + 40 * DAY }, "0,99 €");
      const wrapper = mount(PlanPage, { shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='hint']").text(), state).toBe(en.plan.hint.replace("{price}", "0,99 €"));
      expect(wrapper.find("[data-test='premium']").text(), state).toBe(en.plan.premium);
    }
  });

  // No language keeps the age rule: no text of it, and no key left for one.
  it("has no age rule in any language", () => {
    for (const [path, catalogue] of Object.entries(catalogues)) {
      for (const key of ["young", "iAmYoung", "iAmOlder"]) expect(catalogue.plan, path).not.toHaveProperty(key);
      expect(texts(catalogue.plan).join(" "), path).not.toMatch(/\b21\b/);
    }
  });

  // 2026-09-29: the price is the Store's, as the Store writes it for this phone (0,99 € in
  // Spain, something else elsewhere); the app never writes an amount of its own.
  it("offers the year at the Store's own price when the free year is over", async () => {
    planning({ state: "limited", until: 0 }, "0,99 €");
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("0,99 € a year");
    expect(wrapper.find("[data-test='hint']").text()).toContain("0,99 € a year");
    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_subscribe");
  });

  // Seen on an iPhone (2026-10-08): after the App Store account changed, the screen kept the old
  // storefront's price until a relaunch. The price is asked again whenever the screen is entered,
  // the plan changes or the app comes back; a Store that cannot answer leaves the last one known.
  it("asks the Store for the price again on entering, on a plan change and back on the screen", async () => {
    let price: string | null = "$0.99";
    installTauri((command) => {
      if (command === "core_subscription_price") return { price };
      return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("$0.99 a year");

    price = "0,99 €";
    ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillEnter ?? []).forEach((hook) => hook());
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("0,99 € a year");

    price = "£0.99";
    events.handlers.get("ft://plan")?.();
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("£0.99 a year");

    price = "US$0.99";
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("US$0.99 a year");

    // The Store cannot answer now: the last price known stays, and nothing wrong is said.
    price = null;
    document.dispatchEvent(new Event("visibilitychange"));
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("US$0.99 a year");
    expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
  });

  // Ioan, 2026-10-08: the words he chose, with the Store's price.
  it("says what is free forever and what is premium, with the Store's price", async () => {
    planning({ state: "limited", until: 0 }, "0,99 €");
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='hint']").text()).toBe(
      "Chat, calls, files and games are free, forever. Tools and extra sessions with PIN: free for 15 days, then 0,99 € a year.",
    );
  });

  it("shows another store's price as it comes", async () => {
    planning({ state: "limited", until: 0 }, "US$0.99");
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("US$0.99 a year");
  });

  // Offline, on a desktop, or with the product missing, the Store cannot say: the screen names
  // no amount at all, and paying still goes to the Store, which shows its own price.
  it("names no amount when the Store cannot say the price, and still pays", async () => {
    planning({ state: "limited", until: 0 }, null);
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe(en.plan.payYearly);
    expect(wrapper.find("[data-test='hint']").text()).toBe(en.plan.hintYearly);
    expect(wrapper.text()).not.toMatch(/€|\$|euro/i);
    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_subscribe");
  });

  it("names no amount when asking the Store fails", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscription_price") throw "store_unavailable";
      return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe(en.plan.payYearly);
  });

  // No language may carry a price of its own: an amount written in a catalogue would be wrong in
  // every other store, and the day the price changes.
  it("has no amount of money written in the Plan texts of any language", () => {
    const money = /[€$£¥₹₩₽]|\beuros?\b|ユーロ|유로|欧元|歐元|ยูโร|यूरो|ইউরো|євро|евро|يورو/i;
    for (const [path, catalogue] of Object.entries(catalogues)) {
      for (const text of texts(catalogue.plan)) expect(text, path).not.toMatch(money);
      expect(String(catalogue.plan.pay), path).toContain("{price}");
      expect(String(catalogue.plan.hint), path).toContain("{price}");
    }
  });

  it("asks for nothing in the free year", async () => {
    planning({ state: "trial", until: Date.now() + 40 * DAY });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
    expect(wrapper.find("[data-test='restore']").exists()).toBe(false);
  });

  it("says until when a subscription runs", async () => {
    planning({ state: "subscribed", until: Date.parse("2027-09-23T10:00:00Z") });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("2027");
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
  });

  // 2026-10-07: Google Play never tells the phone until when, so on Android the date kept is only
  // how long the last check of Play holds. The screen says it renews, with no date that means
  // nothing; StoreKit's real expiry (iOS) is still shown.
  it("says a Play subscription renews automatically instead of showing a date", async () => {
    const until = Date.parse("2027-10-07T10:00:00Z");
    planning({ state: "subscribed", until, renews: true });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='where']").text()).toBe(en.plan.renewing);
    expect(wrapper.text()).not.toContain(new Date(until).toLocaleDateString());
  });

  // The Store answers with a key, never with a sentence: what the user reads is translated like
  // everything else, and a key we never wrote never reaches the screen.
  it("says in the user's own words when the Store will not sell", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscribe") throw "not_on_sale";
      return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();

    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.not_on_sale);
  });

  it("keeps an answer nobody wrote off the screen", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscribe") throw "BillingClient exploded at 0x7f";
      return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();

    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.failed);
    expect(wrapper.text()).not.toContain("0x7f");
  });

  // Backing out of the Store window is not a failure: the screen says nothing about it.
  it("says nothing when the user backs out of the Store", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscribe") throw "cancelled";
      return command === "core_plan" ? { state: "limited", until: 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();

    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
  });

  // 2026-10-07: a subscriber on a new phone, or after reinstalling, gets the year they paid for
  // back from the Store without paying again (App Review asks for it, guideline 3.1.1).
  describe("restoring a purchase", () => {
    /** The plan, and what the Store says when asked to restore. */
    function restoring(answer: () => unknown, state = "limited") {
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_restore_subscription") return answer();
        if (command === "core_subscription_price") return { price: "0,99 €" };
        return command === "core_plan" ? { state, until: 0 } : undefined;
      });
    }

    it("is offered once the free year is over", async () => {
      restoring(() => "nothing");
      const wrapper = mount(PlanPage, { shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='restore']").exists()).toBe(true);
    });

    it("is not offered to a subscriber nor in the free year", async () => {
      for (const state of ["subscribed", "trial"]) {
        restoring(() => "nothing", state);
        const wrapper = mount(PlanPage, { shallow: true });
        await flushPromises();
        expect(wrapper.find("[data-test='restore']").exists(), state).toBe(false);
      }
    });

    it("asks the Store and says the subscription is back", async () => {
      restoring(() => "restored");
      const wrapper = mount(PlanPage, { shallow: true });
      await flushPromises();
      calls.length = 0;
      await wrapper.find("[data-test='restore']").trigger("click");
      await flushPromises();
      expect(calls.map(([command]) => command)).toContain("core_restore_subscription");
      expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: en.plan.restored }));
      expect(toast.present).toHaveBeenCalled();
      // And the screen reads the plan again: it says until when, now.
      expect(calls.map(([command]) => command)).toContain("core_plan");
    });

    it("says when the Store has nothing to restore", async () => {
      restoring(() => "nothing");
      const wrapper = mount(PlanPage, { shallow: true });
      await flushPromises();
      await wrapper.find("[data-test='restore']").trigger("click");
      await flushPromises();
      expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: en.plan.nothingToRestore }));
      expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
    });

    it("says a Store that does not answer as it does when paying", async () => {
      restoring(() => {
        throw "store_unavailable";
      });
      const wrapper = mount(PlanPage, { shallow: true });
      await flushPromises();
      await wrapper.find("[data-test='restore']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.store_unavailable);
      expect(toast.create).not.toHaveBeenCalled();
    });
  });

  // Seen in the StoreKit simulator run (2026-10-07): after a parent approved, the screen said
  // "Paid" and still "waiting to be approved". What the Store said before is no longer true.
  it("forgets what the Store said before once the plan changes", async () => {
    let state = "limited";
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscribe") throw "pending_approval";
      return command === "core_plan" ? { state, until: state === "subscribed" ? Date.now() + 300 * DAY : 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").text()).toBe(en.plan.trouble.pending_approval);

    state = "subscribed";
    events.handlers.get("ft://plan")?.();
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
  });

  // 2026-10-07: the Store changes its mind with the screen open (a parent approves an Ask to Buy,
  // the year runs out, a renewal): the core says so and the screen reads the plan again.
  it("follows the plan when the Store changes its mind with the screen open", async () => {
    let state = "limited";
    const until = Date.now() + 300 * DAY;
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscription_price") return { price: "0,99 €" };
      return command === "core_plan" ? { state, until: state === "subscribed" ? until : 0 } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='where']").text()).toBe(en.plan.limited);

    state = "subscribed";
    events.handlers.get("ft://plan")?.();
    await flushPromises();
    expect(wrapper.find("[data-test='where']").text()).toContain(new Date(until).toLocaleDateString());
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);

    wrapper.unmount();
    expect(events.unlisten).toHaveBeenCalled();
  });
});
