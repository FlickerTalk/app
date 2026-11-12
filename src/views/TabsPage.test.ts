import { afterEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import { appsOutline } from "ionicons/icons";
import { defineComponent, h } from "vue";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";
import { childTags, pageShape } from "../__tests__/page-shape";
import { useOnScreen } from "../on-screen";

const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

describe("TabsPage", () => {
  afterEach(() => vi.restoreAllMocks());

  const tabsOf = (wrapper: ReturnType<typeof mount>) => wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));

  it("offers a bottom tab per section: chats, calls, apps and settings", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "apps", "settings"]);
    const apps = wrapper.findAllComponents(IonTabButton)[2];
    expect(apps.attributes("href")).toBe("/tabs/apps");
    expect(apps.attributes("aria-label")).toBe("Apps");
    expect(apps.find("ion-icon-stub").attributes("icon")).toBe(appsOutline);
  });

  it("has the Apps tab on an iPhone too", () => {
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE);
    const wrapper = mount(TabsPage, { shallow: true });
    expect(tabsOf(wrapper)).toEqual(["chats", "calls", "apps", "settings"]);
  });

  it("includes the navigation rail used on wide screens", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(wrapper.findComponent(NavRail).exists()).toBe(true);
  });

  it("is Ionic's tabs page: the rail and ion-tabs, with the outlet and the bottom bar inside", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(pageShape(wrapper)).toEqual(["nav-rail", "ion-tabs"]);
    const tabs = wrapper.find("ion-tabs-stub");
    expect(childTags(tabs.element)).toEqual(["ion-router-outlet", "ion-tab-bar"]);
    expect(tabs.find("ion-tab-bar-stub").attributes("slot")).toBe("bottom");
  });

  it("keeps the tab inside off screen while a page pushed over the tabs covers them", async () => {
    const Tab = defineComponent({
      setup() {
        const onScreen = useOnScreen();
        return () => h("i", { "data-on-screen": String(onScreen.value) });
      },
    });
    const wrapper = mount(TabsPage, { shallow: true, global: { stubs: { IonRouterOutlet: Tab } } });
    const fire = (hook: string) => ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[hook] ?? []).forEach((one) => one());
    const tabOnScreen = async () => {
      await wrapper.vm.$nextTick();
      return wrapper.find("i").attributes("data-on-screen");
    };
    expect(await tabOnScreen()).toBe("true");
    fire("onIonViewWillLeave");
    expect(await tabOnScreen()).toBe("false");
    fire("onIonViewWillEnter");
    fire("onIonViewDidEnter");
    expect(await tabOnScreen()).toBe("true");
  });
});
