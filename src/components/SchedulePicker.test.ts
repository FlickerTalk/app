import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonDatetime } from "@ionic/vue";
import { setLocale } from "../i18n";
import SchedulePicker from "./SchedulePicker.vue";

const picker = (props: Record<string, unknown> = {}) =>
  mount(SchedulePicker, {
    props: { presented: true, sendAt: "2026-10-02T13:00", soonest: "2026-10-02T12:01", latest: "2027-10-02T12:00", weekStart: 1, canSchedule: true, ...props },
  });
const button = (wrapper: ReturnType<typeof picker>, test: string) => wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === test)!;

describe("SchedulePicker", () => {
  it("keeps the picker's room and builds the picker only once its sheet is shown", () => {
    const waiting = picker({ presented: false });
    expect(waiting.find("[data-test='schedule-room']").classes()).toContain("ft-schedule__room");
    expect(waiting.findComponent(IonDatetime).exists()).toBe(false);
    expect(picker().find("[data-test='schedule-room'] [data-test='schedule-at']").exists()).toBe(true);
  });

  it("picks a date and time between the soonest and the latest, from the chosen first day", () => {
    const wrapper = picker();
    const datetime = wrapper.findComponent(IonDatetime);
    expect(datetime.props()).toMatchObject({ presentation: "date-time", modelValue: "2026-10-02T13:00", min: "2026-10-02T12:01", max: "2027-10-02T12:00", firstDayOfWeek: 1, locale: "en" });
    datetime.vm.$emit("update:modelValue", "2026-10-02T15:30");
    expect(wrapper.emitted("pick")).toEqual([["2026-10-02T15:30"]]);
  });

  it("names the time in the app's language", async () => {
    await setLocale("es");
    try {
      const wrapper = picker();
      expect(wrapper.findComponent(IonDatetime).props("locale")).toBe("es");
      expect(wrapper.find("[slot='time-label']").text()).toBe("Hora");
    } finally {
      await setLocale("en");
    }
  });

  it("schedules only when the time can be, and can be dropped", async () => {
    expect(button(picker({ canSchedule: false }), "schedule-go").props("disabled")).toBe(true);
    const wrapper = picker();
    expect(button(wrapper, "schedule-go").props("disabled")).toBe(false);
    await button(wrapper, "schedule-go").trigger("click");
    await button(wrapper, "schedule-cancel").trigger("click");
    expect(wrapper.emitted("schedule")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });
});
