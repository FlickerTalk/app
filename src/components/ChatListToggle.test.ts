import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonIcon } from "@ionic/vue";
import { chevronBackOutline, chevronForwardOutline } from "ionicons/icons";
import ChatListToggle from "./ChatListToggle.vue";

const toggle = (collapsed: boolean) => mount(ChatListToggle, { props: { collapsed, controls: "the-list" }, shallow: true, attachTo: document.body });

describe("ChatListToggle", () => {
  it("says what it will do, and whether the list it controls is expanded", async () => {
    const wrapper = toggle(false);
    const button = wrapper.find("button");
    expect(button.attributes("aria-label")).toBe("Collapse chat list");
    expect(button.attributes("title")).toBe("Collapse chat list");
    expect(button.attributes("aria-expanded")).toBe("true");
    expect(button.attributes("aria-controls")).toBe("the-list");
    await wrapper.setProps({ collapsed: true });
    expect(button.attributes("aria-label")).toBe("Expand chat list");
    expect(button.attributes("title")).toBe("Expand chat list");
    expect(button.attributes("aria-expanded")).toBe("false");
    wrapper.unmount();
  });

  it("points back to collapse and forward to expand, mirrored in right-to-left languages", async () => {
    const wrapper = toggle(false);
    const icon = () => wrapper.findComponent(IonIcon);
    expect(icon().props("icon")).toBe(chevronBackOutline);
    expect(icon().props("flipRtl")).toBe(true);
    await wrapper.setProps({ collapsed: true });
    expect(icon().props("icon")).toBe(chevronForwardOutline);
    wrapper.unmount();
  });

  it("keeps the same focused button when the list changes", async () => {
    const wrapper = toggle(false);
    const button = wrapper.find("button").element as HTMLButtonElement;
    button.focus();
    await wrapper.setProps({ collapsed: true });
    expect(wrapper.find("button").element).toBe(button);
    expect(document.activeElement).toBe(button);
    wrapper.unmount();
  });

  it("asks for the list to change", async () => {
    const wrapper = toggle(false);
    await wrapper.find("button").trigger("click");
    expect(wrapper.emitted("toggle")).toHaveLength(1);
    wrapper.unmount();
  });
});
