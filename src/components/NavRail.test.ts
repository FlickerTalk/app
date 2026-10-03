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

  // 2026-10-03: the games travel inside the app, so an iPad has them too.
  it("has the games entry on an iPad too", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)");
    vi.spyOn(navigator, "maxTouchPoints", "get").mockReturnValue(5);
    const wrapper = mount(NavRail, { shallow: true });
    const labels = wrapper.findAll("button.ft-rail__item").map((button) => button.attributes("aria-label"));
    expect(labels).toEqual(["Chats", "Calls", "Games", "Settings"]);
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
