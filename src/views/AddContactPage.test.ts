import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import AddContactPage from "./AddContactPage.vue";
import QrCode from "../components/QrCode.vue";
import data from "../mock/chats.json";

describe("AddContactPage", () => {
  it("shows my own code so the other phone can scan it", () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    expect(wrapper.findComponent(QrCode).props("value")).toContain(data.me.id);
  });

  it("offers sharing the link instead", () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    expect(wrapper.find("[aria-label='Share link']").exists()).toBe(true);
  });

  it("can switch to scanning the other code", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    expect(wrapper.find("[data-test='scanner']").exists()).toBe(true);
    expect(wrapper.findComponent(QrCode).exists()).toBe(false);
  });
});
