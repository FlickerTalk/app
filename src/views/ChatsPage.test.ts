import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ChatsPage from "./ChatsPage.vue";
import ChatThread from "../components/ChatThread.vue";
import { fixture, seed } from "../__tests__/seed";
import { calls } from "../__tests__/seed";
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

  // Issue app#1: every row opens its contact's own settings.
  it("opens the settings of a contact from its row", async () => {
    const wrapper = mount(ChatsPage, { shallow: true });
    const more = wrapper.findAll("[data-test='chat-more']");
    expect(more).toHaveLength(store.chats.length);
    await more[0].trigger("click");
    expect(push).toHaveBeenCalledWith(`/contact/${store.chats[0].id}`);
  });

  // Hidden sessions: each open session is a foldable panel under the list with its own contacts,
  // a button to add one to it and a button to leave it. No name, no count: nothing to read.
  describe("with an open session", () => {
    beforeEach(() => {
      store.sessions = [
        { id: "s1", chats: [{ ...store.chats[0], id: "ft_pablo", name: "Pablo", unread: 0 }] },
      ];
    });

    it("lists the session's contacts under the main list, under a header with no text", () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      const section = wrapper.find("[data-test='session-section']");
      expect(section.find("header").text()).toBe("");
      expect(section.find("[data-test='session-toggle']").attributes("aria-label")).toBe("Session");
      expect(section.findAll("[data-test='chat-row']")).toHaveLength(1);
      expect(section.text()).toContain("Pablo");
    });

    it("folds and unfolds the session", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      const toggle = wrapper.find("[data-test='session-toggle']");
      expect(toggle.attributes("aria-expanded")).toBe("true");
      await toggle.trigger("click");
      expect(toggle.attributes("aria-expanded")).toBe("false");
      expect(wrapper.find("[data-test='session-section']").findAll("[data-test='chat-row']")).toHaveLength(0);
    });

    it("adds a contact to that session, not to the main list", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-add']").trigger("click");
      expect(push).toHaveBeenCalledWith("/add-contact?session=s1");
    });

    it("leaves the session through the core", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-close']").trigger("click");
      expect(calls).toContainEqual(["core_session_close", { session: "s1" }]);
    });

    it("opens a session's conversation like any other", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-section'] [data-test='chat-row']").trigger("click");
      expect(push).toHaveBeenCalledWith("/chat/ft_pablo");
    });
  });
});
