import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatsPage from "./ChatsPage.vue";
import ChatThread from "../components/ChatThread.vue";
import { fixture, seed } from "../__tests__/seed";
import { store } from "../core";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

function screen(wide: boolean) {
  vi.spyOn(window, "matchMedia").mockReturnValue({
    matches: wide,
    addEventListener: () => undefined,
    removeEventListener: () => undefined,
  } as unknown as MediaQueryList);
}

describe("ChatsPage", () => {
  beforeEach(() => {
    push.mockClear();
    seed();
  });
  afterEach(() => vi.restoreAllMocks());

  it("shows a voice message or a file as such in the list", () => {
    screen(false);
    store.chats[0].lastKind = "voice";
    store.chats[1].lastKind = "file";
    store.chats[1].preview = "menu.pdf";
    const rows = mount(ChatsPage, { shallow: true }).findAll("[data-test='chat-row']");
    expect(rows[0].text()).toContain("Voice message");
    expect(rows[1].find("[aria-label='File']").exists()).toBe(true);
    expect(rows[1].text()).toContain("menu.pdf");
  });

  it("lists every conversation", () => {
    screen(false);
    const wrapper = mount(ChatsPage, { shallow: true });
    expect(wrapper.findAll("[data-test='chat-row']")).toHaveLength(fixture.chats.length);
  });

  it("shows how many messages are unread", () => {
    screen(false);
    const wrapper = mount(ChatsPage, { shallow: true });
    expect(wrapper.find("[data-test='chat-row']").find("[data-test='unread']").text()).toBe("2");
  });

  it("opens the conversation full screen on phones", async () => {
    screen(false);
    const wrapper = mount(ChatsPage, { shallow: true });
    await wrapper.find("[data-test='chat-row']").trigger("click");
    expect(push).toHaveBeenCalledWith("/chat/c1");
  });

  it("opens the screen to add a contact", async () => {
    screen(false);
    const wrapper = mount(ChatsPage, { shallow: true });
    await wrapper.find("[aria-label='Add contact']").trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });

  it("shows the conversation next to the list on wide screens", () => {
    screen(true);
    const wrapper = mount(ChatsPage, { shallow: true });
    expect(wrapper.findComponent(ChatThread).exists()).toBe(true);
  });

  // A new phone has no contacts yet: the list says how to start.
  it("explains how to start when there are no conversations", async () => {
    screen(false);
    store.chats = [];
    const wrapper = mount(ChatsPage, { shallow: true });
    expect(wrapper.find("[data-test='empty']").exists()).toBe(true);
    await wrapper.find("[data-test='empty'] button").trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });
});
