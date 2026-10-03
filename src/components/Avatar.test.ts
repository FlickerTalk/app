import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import Avatar from "./Avatar.vue";

describe("Avatar", () => {
  it("shows the initials of the contact", () => {
    const wrapper = mount(Avatar, { props: { name: "Maria López", hue: 12 } });
    expect(wrapper.text()).toBe("ML");
  });

  it("keeps an emoji whole when a name starts with one", () => {
    const wrapper = mount(Avatar, { props: { name: "😀 Party", hue: 12 } });
    expect(wrapper.text()).toBe("😀P");
  });

  it("keeps a character outside the basic plane whole in a CJK name", () => {
    const wrapper = mount(Avatar, { props: { name: "𠮷田 太郎", hue: 12 } });
    expect(wrapper.text()).toBe("𠮷太");
  });

  it("keeps a flag emoji whole", () => {
    const wrapper = mount(Avatar, { props: { name: "🇪🇸 Spain", hue: 12 } });
    expect(wrapper.text()).toBe("🇪🇸S");
  });

  it("signals a live direct connection", () => {
    const wrapper = mount(Avatar, { props: { name: "Maria López", hue: 12, connected: true } });
    expect(wrapper.find("[aria-label='Directly connected']").exists()).toBe(true);
  });

  it("shows no connection mark when offline", () => {
    const wrapper = mount(Avatar, { props: { name: "Alex Chen", hue: 205 } });
    expect(wrapper.find("[aria-label='Directly connected']").exists()).toBe(false);
  });
});
