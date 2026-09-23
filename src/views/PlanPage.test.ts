import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import PlanPage from "./PlanPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

vi.mock("vue-router", () => ({ useRouter: () => ({ push: vi.fn() }) }));

const DAY = 24 * 60 * 60 * 1000;

/** What the core says about this phone's plan (§40–§47). */
function planning(plan: Record<string, unknown>) {
  installTauri((command, args) => {
    calls.push([command, args]);
    return command === "core_plan" ? plan : undefined;
  });
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

  it("offers the euro when the year is over", async () => {
    planning({ state: "limited", until: 0, age: "adult" });
    const wrapper = mount(PlanPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("1");
    await wrapper.find("[data-test='pay']").trigger("click");
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_subscribe");
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
});
