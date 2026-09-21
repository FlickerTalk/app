import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonRouterOutlet } from "@ionic/vue";
import App from "./App.vue";

describe("App", () => {
  it("hosts the router outlet", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.findComponent(IonRouterOutlet).exists()).toBe(true);
  });

  it("no longer shows the prototype design switcher", () => {
    const wrapper = mount(App, { shallow: true });
    expect(wrapper.find("[aria-label='Design directions (prototype)']").exists()).toBe(false);
  });
});
