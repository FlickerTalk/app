import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import ChatPage from "./ChatPage.vue";
import ChatThread from "../components/ChatThread.vue";
import { askSearch, takeSearch } from "../pending-search";

// The address, reactive as vue-router's is.
const routing = vi.hoisted(() => ({
  route: null as unknown as { params: { id?: string }; query: Record<string, unknown> },
  replace: vi.fn(),
}));
// vue-router's own history: `state.position` is where in the history the page on screen is.
const history = vi.hoisted(() => ({ state: { position: 3 } as Record<string, unknown> | null }));
vi.mock("vue-router", async () => {
  const { reactive } = await import("vue");
  routing.route = reactive({ params: { id: "c2" }, query: {} });
  return { useRoute: () => routing.route, useRouter: () => ({ options: { history }, replace: routing.replace }) };
});

describe("ChatPage", () => {
  it("shows the conversation from the route, with a way back", () => {
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("chatId")).toBe("c2");
    expect(thread.props("showBack")).toBe(true);
    expect(thread.props("play")).toBeUndefined();
  });

  // Ionic gives each conversation a page of its own and keeps it mounted while another page is
  // on top or while it goes: the address then is the other page's, never this one's conversation.
  // Following it emptied the conversation (and took down its game) at once when going back.
  it("keeps its own conversation when the address moves on", async () => {
    routing.route.params = { id: "c2" };
    const wrapper = mount(ChatPage, { shallow: true });
    routing.route.params = {};
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(ChatThread).props("chatId")).toBe("c2");
    routing.route.params = { id: "c3" };
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(ChatThread).props("chatId")).toBe("c2");
    routing.route.params = { id: "c2" };
  });

  // Plan 10.4: the games tab opens a game in the conversation with the contact picked.
  it("hands the conversation the game the address asks to play", () => {
    routing.route.query = { play: "com.flickertalk.game.chess" };
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("play")).toBe("com.flickertalk.game.chess");
  });

  // Ioan, 2026-10-06: the contact page's search opens the conversation with `?search=1`. The
  // address then forgets it, so going back and forth does not open the search again.
  it("opens the search the address asks for, and takes it out of the address", () => {
    routing.route.query = { search: "1" };
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("search")).toBe(true);
    expect(routing.replace).toHaveBeenCalledWith({ query: {} });
    routing.route.query = {};
  });

  // Ionic keeps a conversation's page mounted under the next one: the search another
  // conversation's address asks for is not this one's.
  it("leaves the search to the conversation the address is for", async () => {
    routing.route.params = { id: "c2" };
    routing.route.query = {};
    const wrapper = mount(ChatPage, { shallow: true });
    routing.replace.mockClear();
    routing.route.params = { id: "c3" };
    routing.route.query = { search: "1" };
    await wrapper.vm.$nextTick();
    expect(wrapper.findComponent(ChatThread).props("search")).toBe(false);
    expect(routing.replace).not.toHaveBeenCalled();
    routing.route.params = { id: "c2" };
    routing.route.query = {};
  });

  it("opens no search when the address does not ask for it", () => {
    routing.route.query = {};
    routing.replace.mockClear();
    const thread = mount(ChatPage, { shallow: true }).findComponent(ChatThread);
    expect(thread.props("search")).toBe(false);
    expect(routing.replace).not.toHaveBeenCalled();
  });

  // Ioan, 2026-10-06: as in WhatsApp, the contact page's search goes back to the conversation it
  // came from; the conversation opens its search once it is on screen again.
  describe("back from the contact page's search", () => {
    const openSearch = vi.fn();
    const Thread = defineComponent({
      name: "ChatThread",
      props: ["chatId", "showBack", "play", "search", "active"],
      setup(_, { expose }) {
        expose({ leave: vi.fn(), openSearch });
        return () => h("div");
      },
    });
    const entered = (wrapper: { vm: unknown }) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewDidEnter ?? []).forEach((hook) => hook());

    it("opens the search asked for this conversation when it is on screen again", () => {
      openSearch.mockClear();
      routing.route.params = { id: "c2" };
      const wrapper = mount(ChatPage, { shallow: true, global: { stubs: { ChatThread: Thread } } });
      askSearch("c2");
      entered(wrapper);
      expect(openSearch).toHaveBeenCalledTimes(1);
      expect(takeSearch("c2")).toBe(false);
      entered(wrapper);
      expect(openSearch).toHaveBeenCalledTimes(1);
    });

    it("leaves another conversation's search alone", () => {
      openSearch.mockClear();
      routing.route.params = { id: "c2" };
      const wrapper = mount(ChatPage, { shallow: true, global: { stubs: { ChatThread: Thread } } });
      askSearch("c3");
      entered(wrapper);
      expect(openSearch).not.toHaveBeenCalled();
      expect(takeSearch("c3")).toBe(true);
    });
  });

  // 2026-10-02: left by going back, the page is taken down once Ionic's transition ends, and with
  // it the plugin or game open in it. It is closed as the page starts to go, so its goodbye goes
  // out meanwhile. A page only covered by another one (a push) keeps it, as before.
  describe("leaving", () => {
    const leave = vi.fn();
    const Thread = defineComponent({
      name: "ChatThread",
      props: ["chatId", "showBack", "play", "active"],
      setup(_, { expose }) {
        expose({ leave });
        return () => h("div");
      },
    });
    const hooks = (wrapper: { vm: unknown }, name: string) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>)[name] ?? []).forEach((hook) => hook());

    /** The page, entered at history position 3. */
    function page() {
      leave.mockClear();
      history.state = { position: 3 };
      const wrapper = mount(ChatPage, { shallow: true, global: { stubs: { ChatThread: Thread } } });
      hooks(wrapper, "onIonViewWillEnter");
      return wrapper;
    }

    it("closes what is open in the conversation when the page is left by going back", () => {
      const wrapper = page();
      history.state = { position: 2 };
      hooks(wrapper, "onIonViewWillLeave");
      expect(leave).toHaveBeenCalledTimes(1);
    });

    it("keeps it when another page is pushed over this one", () => {
      const wrapper = page();
      history.state = { position: 4 };
      hooks(wrapper, "onIonViewWillLeave");
      expect(leave).not.toHaveBeenCalled();
    });

    // A back button with no history behind it replaces the page (`defaultHref`): the position does
    // not go down, so nothing is closed here; the page's teardown is the plugin's only word then.
    it("closes nothing when the page is replaced at the same position", () => {
      const wrapper = page();
      hooks(wrapper, "onIonViewWillLeave");
      expect(leave).not.toHaveBeenCalled();
    });

    // Back on screen after a pushed page: the position it came back to is the one that counts.
    it("counts from where the page was entered last", () => {
      const wrapper = page();
      history.state = { position: 4 };
      hooks(wrapper, "onIonViewWillLeave");
      history.state = { position: 2 };
      hooks(wrapper, "onIonViewWillEnter");
      history.state = { position: 1 };
      hooks(wrapper, "onIonViewWillLeave");
      expect(leave).toHaveBeenCalledTimes(1);
    });

    it("closes nothing when the history says no position", () => {
      const wrapper = page();
      history.state = null;
      hooks(wrapper, "onIonViewWillLeave");
      expect(leave).not.toHaveBeenCalled();
    });
  });
});
