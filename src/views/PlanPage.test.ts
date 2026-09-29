import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import PlanPage from "./PlanPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import en from "../i18n/en.json";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

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
  beforeEach(seed);

  it("says how long the free year has left", async () => {
    planning({ state: "trial", until: Date.now() + 40 * DAY, age: "unknown" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("40");
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
  });

  // The same free year reads the same on Settings (`daysLeft`): installed a moment ago, 365 days.
  it("counts the free year as Settings does", async () => {
    planning({ state: "trial", until: Date.now() + 365 * DAY - 5, age: "unknown" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='where']").text()).toBe("Free · 365 days left");
  });

  // §40: under 21 it is free, and that is a thing the user says, never a date we keep.
  it("asks the age and keeps only the answer", async () => {
    planning({ state: "limited", until: 0, age: "unknown" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();

    await wrapper.find("[data-test='young']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_set_age", { age: "minor" }]);
    expect(calls.some(([command]) => command === "core_plan")).toBe(true);
    expect(wrapper.html()).not.toContain("birth");
  });

  // 2026-09-29: the price is the Store's, as the Store writes it for this phone (0,99 € in
  // Spain, something else elsewhere); the app never writes an amount of its own.
  it("offers the year at the Store's own price when the free year is over", async () => {
    planning({ state: "limited", until: 0, age: "adult" }, "0,99 €");
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("0,99 € a year");
    expect(wrapper.find("[data-test='hint']").text()).toContain("0,99 € a year");
    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_subscribe");
  });

  it("shows another store's price as it comes", async () => {
    planning({ state: "limited", until: 0, age: "adult" }, "US$0.99");
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").text()).toBe("US$0.99 a year");
  });

  // Offline, on a desktop, or with the product missing, the Store cannot say: the screen names
  // no amount at all, and paying still goes to the Store, which shows its own price.
  it("names no amount when the Store cannot say the price, and still pays", async () => {
    planning({ state: "limited", until: 0, age: "adult" }, null);
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
      return command === "core_plan" ? { state: "limited", until: 0, age: "adult" } : undefined;
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

  it("asks for nothing from someone under 21", async () => {
    planning({ state: "young", until: 0, age: "minor" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
  });

  it("says until when a subscription runs", async () => {
    planning({ state: "subscribed", until: Date.parse("2027-09-23T10:00:00Z"), age: "adult" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("2027");
    expect(wrapper.find("[data-test='pay']").exists()).toBe(false);
  });

  // The Store answers with a key, never with a sentence: what the user reads is translated like
  // everything else, and a key we never wrote never reaches the screen.
  it("says in the user's own words when the Store will not sell", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_subscribe") throw "not_on_sale";
      return command === "core_plan" ? { state: "limited", until: 0, age: "adult" } : undefined;
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
      return command === "core_plan" ? { state: "limited", until: 0, age: "adult" } : undefined;
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
      return command === "core_plan" ? { state: "limited", until: 0, age: "adult" } : undefined;
    });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();

    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='trouble']").exists()).toBe(false);
  });
});
