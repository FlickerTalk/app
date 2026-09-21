import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import CallsPage from "./CallsPage.vue";
import data from "../mock/chats.json";

describe("CallsPage", () => {
  it("lists the recent calls, each with a way to call back", () => {
    const wrapper = mount(CallsPage, { shallow: true });
    expect(wrapper.findAll("[data-test='call-row']")).toHaveLength(data.calls.length);
    expect(wrapper.findAll("[aria-label='Call back']")).toHaveLength(data.calls.length);
  });

  it("marks missed calls", () => {
    const wrapper = mount(CallsPage, { shallow: true });
    expect(wrapper.findAll(".is-missed")).toHaveLength(1);
  });
});
