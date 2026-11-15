import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatsEmpty from "./ChatsEmpty.vue";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

const empty = (compact: boolean) => mount(ChatsEmpty, { props: { compact }, shallow: true });

describe("ChatsEmpty", () => {
  beforeEach(() => push.mockClear());

  it("explains how to start and leads to adding a contact", async () => {
    const wrapper = empty(false);
    expect(wrapper.text()).toContain("No chats yet");
    await wrapper.find("button").trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });

  it("offers the quick tour as often as asked", async () => {
    const wrapper = empty(false);
    const link = wrapper.find("[data-test='tour-link']");
    expect(link.html()).toContain("How FlickerTalk works");
    await link.trigger("click");
    await link.trigger("click");
    expect(wrapper.emitted("tour")).toHaveLength(2);
    expect(push).not.toHaveBeenCalled();
  });

  it("in the strip, keeps only a named add-contact button", async () => {
    const wrapper = empty(true);
    expect(wrapper.text()).toBe("");
    expect(wrapper.find("[data-test='tour-link']").exists()).toBe(false);
    const buttons = wrapper.findAll("[data-test='empty-add']");
    expect(buttons).toHaveLength(1);
    expect(buttons[0].attributes("aria-label")).toBe("Add contact");
    await buttons[0].trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });
});
