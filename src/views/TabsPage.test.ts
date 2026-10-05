import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";
import { setExtraTab } from "../preferences";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

describe("TabsPage", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    setExtraTab("none");
  });

  const tabsOf = (wrapper: ReturnType<typeof mount>) => wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));

  // 2026-10-05 (Ioan): out of the box the bar has three tabs; the fourth is Settings' choice.
  it("offers a bottom tab per section, and none for the games or the plugins until asked", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "settings"]);
  });

  // Plan 10.3: the games have a tab of their own, between Calls and Settings.
  it("puts the games between Calls and Settings when Settings says so", () => {
    setExtraTab("games");
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "games", "settings"]);
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("href")).toBe("/tabs/games");
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("aria-label")).toBe("Games");
  });

  it("puts the plugins there instead when that is the choice", () => {
    setExtraTab("plugins");
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "plugins", "settings"]);
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("href")).toBe("/tabs/plugins");
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("aria-label")).toBe("Plugins");
  });

  // Settings is itself a tab: the bar changes under it, with no restart.
  it("follows the choice while it is on screen", async () => {
    const wrapper = mount(TabsPage, { shallow: true });
    setExtraTab("games");
    await wrapper.vm.$nextTick();
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "games", "settings"]);
    setExtraTab("none");
    await wrapper.vm.$nextTick();
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "settings"]);
  });

  // 2026-10-03: the games travel inside the app, so an iPhone, which downloads nothing (App Store
  // 4.7, §52), has the games tab too.
  it("has the games tab on an iPhone too", () => {
    setExtraTab("games");
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE);
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "games", "settings"]);
  });

  it("includes the navigation rail used on wide screens", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(wrapper.findComponent(NavRail).exists()).toBe(true);
  });
});
