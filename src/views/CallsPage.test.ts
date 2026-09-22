import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CallsPage from "./CallsPage.vue";

// Calls arrive with milestone M6 (Plan §106); until then the history is empty, never invented.
describe("CallsPage", () => {
  it("shows that there are no calls yet", () => {
    const wrapper = mount(CallsPage, { shallow: true });
    expect(wrapper.findAll("[data-test='call-row']")).toHaveLength(0);
    expect(wrapper.find("[data-test='empty']").exists()).toBe(true);
  });
});
