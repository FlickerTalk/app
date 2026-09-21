import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import NavRail from "./NavRail.vue";

const push = vi.fn();
vi.mock("vue-router", () => ({
  useRoute: () => ({ path: "/tabs/calls" }),
  useRouter: () => ({ push }),
}));

describe("NavRail", () => {
  beforeEach(() => push.mockClear());

  it("offers one entry per section", () => {
    const wrapper = mount(NavRail, { shallow: true });
    for (const label of ["Chats", "Calls", "Settings"]) {
      expect(wrapper.find(`button[aria-label='${label}']`).exists()).toBe(true);
    }
  });

  it("marks the current section", () => {
    const wrapper = mount(NavRail, { shallow: true });
    expect(wrapper.find("[aria-current='page']").attributes("aria-label")).toBe("Calls");
  });

  it("navigates to the chosen section", async () => {
    const wrapper = mount(NavRail, { shallow: true });
    await wrapper.find("button[aria-label='Chats']").trigger("click");
    expect(push).toHaveBeenCalledWith("/tabs/chats");
  });
});
