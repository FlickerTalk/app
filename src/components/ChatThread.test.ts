import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import data from "../mock/chats.json";

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

  it("has a composer to attach and send", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Attach file']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Send']").exists()).toBe(true);
  });
});
