import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import NavRail from "./NavRail.vue";

const push = vi.fn();
vi.mock("vue-router", () => ({
  useRoute: () => ({ path: "/tabs/calls" }),
  useRouter: () => ({ push }),
}));

describe("NavRail", () => {
  beforeEach(() => {
    push.mockClear();
    seed();
  });

  afterEach(() => vi.restoreAllMocks());

  it("offers one entry per section", () => {
    const wrapper = mount(NavRail, { shallow: true });
    const labels = wrapper.findAll("button.ft-rail__item").map((button) => button.attributes("aria-label"));
    expect(labels).toEqual(["Chats", "Calls", "Games", "Settings"]);
  });

  it("goes to the games", async () => {
    const wrapper = mount(NavRail, { shallow: true });
    await wrapper.find("button[aria-label='Games']").trigger("click");
    expect(push).toHaveBeenCalledWith("/tabs/games");
  });

  // No downloads on iOS, so no games there (plan 10.3).
  it("has no games entry on an iPhone", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)");
    const wrapper = mount(NavRail, { shallow: true });
    const labels = wrapper.findAll("button.ft-rail__item").map((button) => button.attributes("aria-label"));
    expect(labels).toEqual(["Chats", "Calls", "Settings"]);
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
