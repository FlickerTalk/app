import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
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

  // Circles (2026-09-27): a row like any other, with who said the last thing, and a button to
  // make a new one, which needs someone to put in it.
  describe("with a circle", () => {
    beforeEach(() => {
      store.circles = [
        {
          id: "circle1",
          name: "Friends",
          hue: 120,
          members: [
            { id: "ft_me", name: "Me", admin: true, me: true },
            { id: "c1", name: "Maria López", admin: false, me: false },
          ],
          admin: true,
          adminsOnly: false,
          left: false,
          unread: 3,
          time: "10:02",
          preview: "dinner on friday?",
          lastMine: false,
          lastSender: "Maria López",
          status: "delivered",
          messages: [{ id: "m1", mine: false, text: "dinner on friday?", time: "10:02", kind: "text", sender: "c1", senderName: "Maria López" }],
        },
      ];
    });

    it("lists the circle with who said the last thing and how much is unread", () => {
      screen(false);
      const row = mount(ChatsPage, { shallow: true }).find("[data-test='circle-row']");
      expect(row.text()).toContain("Friends");
      expect(row.text()).toContain("Maria López: dinner on friday?");
      expect(row.find("[data-test='unread']").text()).toBe("3");
    });

    it("opens the circle full screen on phones and next to the list on wide screens", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='circle-row']").trigger("click");
      expect(push).toHaveBeenCalledWith("/circle/circle1");
      await wrapper.find("[data-test='circle-more']").trigger("click");
      expect(push).toHaveBeenCalledWith("/circle/circle1/info");

      screen(true);
      const wide = mount(ChatsPage, { shallow: true });
      await wide.find("[data-test='circle-row']").trigger("click");
      expect(wide.findComponent({ name: "CircleThread" }).exists()).toBe(true);
    });

    it("offers a new circle only when there is someone to put in it", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='new-circle']").trigger("click");
      expect(push).toHaveBeenCalledWith("/new-circle");
      store.chats = [];
      expect(mount(ChatsPage, { shallow: true }).find("[data-test='new-circle']").exists()).toBe(false);
    });
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
        { id: "s1", chats: [{ ...store.chats[0], id: "ft_pablo", name: "Pablo", unread: 0 }], requests: [], circles: [] },
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

    // A3: a session can go for good; the trash asks once and only then the core is told.
    it("deletes the session after asking once", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-remove']").trigger("click");
      expect(calls.some(([command]) => command === "core_session_remove")).toBe(false);
      await wrapper.find("[data-test='session-remove-sure']").trigger("click");
      expect(calls).toContainEqual(["core_session_remove", { session: "s1" }]);
      expect(store.sessions).toHaveLength(0);
    });

    // A5: whoever scanned the session's QR waits in its own requests; the yes and the no are in
    // the conversation, as with the main list's requests (2026-09-28).
    it("shows the session's requests, which open like any conversation", async () => {
      screen(false);
      store.sessions[0].requests = [{ ...store.chats[1], id: "ft_stranger", name: "Someone", unread: 1 }];
      const wrapper = mount(ChatsPage, { shallow: true });
      const rows = wrapper.findAll("[data-test='session-requests'] [data-test='request-row']");
      expect(rows).toHaveLength(1);
      expect(rows[0].find("[data-test='request-accept']").exists()).toBe(false);
      expect(rows[0].find("[data-test='request-decline']").exists()).toBe(false);
      await rows[0].find("button").trigger("click");
      expect(push).toHaveBeenCalledWith("/chat/ft_stranger");
    });
  });

  // A3: with the seven slots taken, a new PIN shows an empty session that is only on the screen.
  // It looks like any other, trash included; it adds nobody, and it goes without telling the core.
  describe("with a session that is only on the screen", () => {
    beforeEach(() => {
      store.sessions = [{ id: "", pin: "135790", chats: [], requests: [], circles: [] }];
    });

    it("looks like any other session, trash included", () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      expect(wrapper.find("[data-test='session-add']").exists()).toBe(true);
      expect(wrapper.find("[data-test='session-remove']").exists()).toBe(true);
    });

    it("adds nobody, as there is no room", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-add']").trigger("click");
      await flushPromises();
      expect(push).not.toHaveBeenCalled();
      expect(calls.some(([command]) => command.startsWith("core_session"))).toBe(false);
    });

    it("goes with the trash without telling the core", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      await wrapper.find("[data-test='session-remove']").trigger("click");
      await wrapper.find("[data-test='session-remove-sure']").trigger("click");
      await flushPromises();
      expect(calls.some(([command]) => command.startsWith("core_session"))).toBe(false);
      expect(store.sessions).toHaveLength(0);
    });
  });

  // A5: strangers who wrote first with this phone's link wait apart from the list.
  describe("with requests", () => {
    beforeEach(() => {
      store.requests = [{ ...store.chats[1], id: "ft_stranger12345", name: "Mamá", unread: 1, preview: "hey" }];
    });

    it("shows them apart, with a short id next to the name", () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      const panel = wrapper.find("[data-test='requests']");
      expect(panel.exists()).toBe(true);
      expect(panel.findAll("[data-test='request-row']")).toHaveLength(1);
      expect(panel.text()).toContain("Mamá");
      expect(panel.text()).toContain("ft_strang");
      expect(panel.text()).toContain("hey");
      // The main list is the main list: the request is not among its rows.
      expect(wrapper.findAll("[data-test='chat-row']")).toHaveLength(fixture.chats.length);
    });

    // Seen on the Lenovo (2026-09-28): a yes and a no on the row, side by side, and on a small
    // screen the no is easy to hit by mistake. As in WhatsApp, they live in the conversation.
    it("has no yes or no on the row: it opens the conversation, where they are", async () => {
      screen(false);
      const wrapper = mount(ChatsPage, { shallow: true });
      expect(wrapper.find("[data-test='request-accept']").exists()).toBe(false);
      expect(wrapper.find("[data-test='request-decline']").exists()).toBe(false);
      await wrapper.find("[data-test='request-row'] button").trigger("click");
      expect(push).toHaveBeenCalledWith("/chat/ft_stranger12345");
      expect(calls.some(([command]) => command === "core_decline_contact" || command === "core_accept_contact")).toBe(false);
    });

    it("is not an empty phone while a request waits", () => {
      screen(false);
      store.chats = [];
      const wrapper = mount(ChatsPage, { shallow: true });
      expect(wrapper.find("[data-test='empty']").exists()).toBe(false);
      expect(wrapper.find("[data-test='requests']").exists()).toBe(true);
    });
  });
});
