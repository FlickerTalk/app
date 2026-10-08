import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import { appsOutline } from "ionicons/icons";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";
import { childTags, pageShape } from "../__tests__/page-shape";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

describe("TabsPage", () => {
  afterEach(() => vi.restoreAllMocks());

  const tabsOf = (wrapper: ReturnType<typeof mount>) => wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));

  // 2026-10-08 (plan of the apps grid): four fixed tabs; the tools and the games are the Apps tab,
  // between Calls and Settings. There is no choice of a fourth tab any more.
  it("offers a bottom tab per section: chats, calls, apps and settings", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "apps", "settings"]);
    const apps = wrapper.findAllComponents(IonTabButton)[2];
    expect(apps.attributes("href")).toBe("/tabs/apps");
    expect(apps.attributes("aria-label")).toBe("Apps");
    expect(apps.find("ion-icon-stub").attributes("icon")).toBe(appsOutline);
  });

  // 2026-10-03: the games and the small tools travel inside the app, so an iPhone, which downloads
  // nothing (App Store 4.7, §52), has the Apps tab too.
  it("has the Apps tab on an iPhone too", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE);
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "apps", "settings"]);
  });

  it("includes the navigation rail used on wide screens", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(wrapper.findComponent(NavRail).exists()).toBe(true);
  });

  // Ionic's own shape (2026-10-09): the page holds the tabs themselves, as Ionic's transitions look
  // for them (`:scope > ion-tabs`); with a frame of ours in between the tabs never slid away on the
  // iPhone. The rail sits beside them, and the tabs hold the outlet and the bar.
  it("is Ionic's tabs page: the rail and ion-tabs, with the outlet and the bottom bar inside", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(pageShape(wrapper)).toEqual(["nav-rail", "ion-tabs"]);
    const tabs = wrapper.find("ion-tabs-stub");
    expect(childTags(tabs.element)).toEqual(["ion-router-outlet", "ion-tab-bar"]);
    expect(tabs.find("ion-tab-bar-stub").attributes("slot")).toBe("bottom");
  });
});
