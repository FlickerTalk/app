import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

describe("TabsPage", () => {
  afterEach(() => vi.restoreAllMocks());

  // Plan 10.3: the games have a tab of their own, between Calls and Settings.
  it("offers a bottom tab per section", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    const tabs = wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));
    expect(tabs).toEqual(["chats", "calls", "games", "settings"]);
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("href")).toBe("/tabs/games");
    expect(wrapper.findAllComponents(IonTabButton)[2].attributes("aria-label")).toBe("Games");
  });

  // No downloads on iOS, so nothing to put in it (App Store 4.7, §52).
  it("has no games tab on an iPhone", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE);
    const wrapper = mount(TabsPage, { shallow: true });
    const tabs = wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));
    expect(tabs).toEqual(["chats", "calls", "settings"]);
  });

  it("includes the navigation rail used on wide screens", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(wrapper.findComponent(NavRail).exists()).toBe(true);
  });
});
