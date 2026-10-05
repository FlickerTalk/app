import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import { setExtraTab } from "../preferences";
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

  afterEach(() => {
    vi.restoreAllMocks();
    setExtraTab("none");
  });

  const labelsOf = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAll("button.ft-rail__item").map((button) => button.attributes("aria-label"));

  // 2026-10-05 (Ioan): the rail mirrors the tab bar: three entries unless Settings adds a fourth.
  it("offers one entry per section, and none for the games or the plugins until asked", () => {
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Settings"]);
  });

  it("goes to the games when Settings puts them on the bar", async () => {
    setExtraTab("games");
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Games", "Settings"]);
    await wrapper.find("button[aria-label='Games']").trigger("click");
    expect(push).toHaveBeenCalledWith("/tabs/games");
  });

  it("goes to the plugins when they are the choice", async () => {
    setExtraTab("plugins");
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Plugins", "Settings"]);
    await wrapper.find("button[aria-label='Plugins']").trigger("click");
    expect(push).toHaveBeenCalledWith("/tabs/plugins");
  });

  it("follows the choice while it is on screen", async () => {
    const wrapper = mount(NavRail, { shallow: true });
    setExtraTab("plugins");
    await wrapper.vm.$nextTick();
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Plugins", "Settings"]);
  });

  // 2026-10-03: the games travel inside the app, so an iPad has them too.
  it("has the games entry on an iPad too", () => {
    setExtraTab("games");
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)");
    vi.spyOn(navigator, "maxTouchPoints", "get").mockReturnValue(5);
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Games", "Settings"]);
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
