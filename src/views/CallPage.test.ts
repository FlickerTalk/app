import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import CallPage from "./CallPage.vue";

const route = { params: { id: "c1" }, query: {} as Record<string, string> };
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => ({ back: vi.fn() }) }));

describe("CallPage", () => {
  it("shows who is being called and how to hang up", () => {
    route.query = {};
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    expect(wrapper.find("[aria-label='Hang up']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Mute']").exists()).toBe(true);
  });

  it("shows the video area only on video calls", () => {
    route.query = {};
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(false);
    route.query = { video: "1" };
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(true);
  });
});
