import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import data from "../mock/chats.json";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

describe("ChatThread", () => {
  it("shows every message of the conversation", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.findAllComponents(MessageBubble)).toHaveLength(data.chats[0].messages.length);
  });

  it("offers voice and video calls", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Voice call']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Video call']").exists()).toBe(true);
  });

  it("tells whether the contact is directly connected", () => {
    expect(mount(ChatThread, { props: { chatId: "c1" }, shallow: true }).text()).toContain("Direct");
    expect(mount(ChatThread, { props: { chatId: "c2" }, shallow: true }).text()).toContain("Not connected");
  });

  it("starts a voice or a video call", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Voice call']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/c1");
    await wrapper.find("[aria-label='Video call']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/c1?video=1");
  });

  it("opens the contact details from the header", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[data-test='peer']").trigger("click");
    expect(push).toHaveBeenCalledWith("/contact/c1");
  });

  it("has a composer to attach and send", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Attach file']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Send']").exists()).toBe(true);
  });
});
