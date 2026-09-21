import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatsPage from "./ChatsPage.vue";
import ChatThread from "../components/ChatThread.vue";
import data from "../mock/chats.json";

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
  beforeEach(() => push.mockClear());
  afterEach(() => vi.restoreAllMocks());

  it("lists every conversation", () => {
    screen(false);
    const wrapper = mount(ChatsPage, { shallow: true });
    expect(wrapper.findAll("[data-test='chat-row']")).toHaveLength(data.chats.length);
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
});
