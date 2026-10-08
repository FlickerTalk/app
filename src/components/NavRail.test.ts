import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import NavRail from "./NavRail.vue";
import source from "./NavRail.vue?raw";

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

  const labelsOf = (wrapper: ReturnType<typeof mount>) =>
    wrapper.findAll("button.ft-rail__item").map((button) => button.attributes("aria-label"));

  // 2026-10-08 (plan of the apps grid): the rail mirrors the tab bar: four fixed entries.
  it("offers one entry per section: chats, calls, apps and settings", () => {
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Apps", "Settings"]);
  });

  it("goes to the Apps tab", async () => {
    const wrapper = mount(NavRail, { shallow: true });
    await wrapper.find("button[aria-label='Apps']").trigger("click");
    expect(push).toHaveBeenCalledWith("/tabs/apps");
  });

  // 2026-10-03: the games travel inside the app, so an iPad has them too.
  it("has the Apps entry on an iPad too", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)");
    vi.spyOn(navigator, "maxTouchPoints", "get").mockReturnValue(5);
    const wrapper = mount(NavRail, { shallow: true });
    expect(labelsOf(wrapper)).toEqual(["Chats", "Calls", "Apps", "Settings"]);
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

  // Device review of app#121: on Android the hover pill stayed after a touch. It is for a pointer
  // that hovers only.
  it("shows the hover pill only where the pointer can hover", () => {
    const styles = source.slice(source.indexOf("<style"));
    const hover = styles.indexOf(".ft-rail__item:hover");
    expect(hover).toBeGreaterThan(0);
    const before = styles.slice(0, hover);
    expect(before).toMatch(/@media \(hover: hover\) \{\s*$/);
  });
});
