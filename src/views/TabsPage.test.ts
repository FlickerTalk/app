import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import { appsOutline } from "ionicons/icons";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";

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
});
