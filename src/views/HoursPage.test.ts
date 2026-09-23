import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonToggle } from "@ionic/vue";
import HoursPage from "./HoursPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

vi.mock("vue-router", () => ({ useRouter: () => ({ back: vi.fn() }) }));

/** The last weekly hours sent to the core. */
function saved() {
  const last = calls.filter(([command]) => command === "core_set_quiet_hours").pop();
  return last?.[1]?.hours === null ? null : JSON.parse(String(last?.[1]?.hours));
}

// Issue app#7: off by default; on, seven days, each all day, never, or a stretch.
describe("HoursPage", () => {
  beforeEach(() => seed());

  it("starts off, with no days shown", async () => {
    const wrapper = mount(HoursPage, { shallow: true });
    await flushPromises();
    expect(wrapper.findComponent(IonToggle).attributes("checked")).toBe("false");
    expect(wrapper.findAll("[data-test='day']")).toHaveLength(0);
  });

  it("turning it on shows seven days, all day, and saves them", async () => {
    const wrapper = mount(HoursPage, { shallow: true });
    await flushPromises();
    wrapper.findComponent(IonToggle).vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    expect(wrapper.findAll("[data-test='day']")).toHaveLength(7);
    expect(saved()).toEqual({ days: ["all", "all", "all", "all", "all", "all", "all"] });
  });

  it("gives a day a stretch of hours, or none", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_quiet_hours" ? JSON.stringify({ days: ["all", "all", "all", "all", "all", "all", "all"] }) : undefined;
    });
    const wrapper = mount(HoursPage, { shallow: true });
    await flushPromises();
    const friday = wrapper.findAll("[data-test='day']")[4];
    await friday.find("[data-test='mode-hours']").trigger("click");
    await flushPromises();
    await friday.find("[data-test='from']").setValue("15:00");
    await friday.find("[data-test='to']").setValue("22:00");
    await flushPromises();
    expect(saved().days[4]).toEqual({ from: "15:00", to: "22:00" });
    await wrapper.findAll("[data-test='day']")[6].find("[data-test='mode-none']").trigger("click");
    await flushPromises();
    expect(saved().days[6]).toBe("none");
  });

  it("turning it off forgets the hours", async () => {
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_quiet_hours" ? JSON.stringify({ days: ["all", "all", "all", "all", "all", "all", "all"] }) : undefined;
    });
    const wrapper = mount(HoursPage, { shallow: true });
    await flushPromises();
    wrapper.findComponent(IonToggle).vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: false } }));
    await flushPromises();
    expect(saved()).toBeNull();
  });
});
