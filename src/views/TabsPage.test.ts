import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonTabButton } from "@ionic/vue";
import TabsPage from "./TabsPage.vue";
import NavRail from "../components/NavRail.vue";

describe("TabsPage", () => {
  it("offers a bottom tab per section", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    const tabs = wrapper.findAllComponents(IonTabButton).map((tab) => tab.attributes("tab"));
    expect(tabs).toEqual(["chats", "calls", "settings"]);
  });

  it("includes the navigation rail used on wide screens", () => {
    const wrapper = mount(TabsPage, { shallow: true });
    expect(wrapper.findComponent(NavRail).exists()).toBe(true);
  });
});
