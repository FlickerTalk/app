import { afterEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h } from "vue";
import { store } from "../core/store";
import type { OpenPlugin } from "../pluginWindow";
import PluginWindow from "./PluginWindow.vue";

const close = vi.fn(async () => {});
const SheetStub = defineComponent({
  name: "PluginSheet",
  props: {
    plugin: { type: Object, default: undefined },
    contact: { type: String, default: "" },
    sending: { type: String, default: "nothing" },
    live: Boolean,
    text: { type: String, default: undefined },
    file: { type: Object, default: undefined },
    reference: { type: String, default: undefined },
    session: { type: String, default: undefined },
    fill: Boolean,
  },
  emits: ["text", "attach", "openChat", "done", "closed", "needs"],
  setup(_, { expose }) {
    expose({ close });
    return () => h("iframe");
  },
});

const TOOL: OpenPlugin = { id: "com.flickertalk.code", name: "Code block", sending: "nothing", live: false, text: "# Title", reference: "ref_1", contact: "c1", opening: 1 };
const GAME: OpenPlugin = { id: "com.flickertalk.game.chess", name: "Chess", sending: "propose", live: true, game: true, contact: "c1", opening: 2 };

const windowFor = (plugin: OpenPlugin, props: Record<string, unknown> = {}) =>
  mount(PluginWindow, { props: { plugin, peeking: false, session: "s1", ...props }, global: { stubs: { IonIcon: true, PluginSheet: SheetStub } } });

enableAutoUnmount(afterEach);

describe("PluginWindow", () => {
  it("opens a tool in its own full-screen window with a way back and the tool's name", async () => {
    const wrapper = windowFor(TOOL);
    const window = wrapper.find(".ft-app");
    expect(window.attributes("role")).toBe("dialog");
    expect(window.attributes("aria-label")).toBe("Code block");
    expect(wrapper.find("[data-test='app-bar'] .ft-title").text()).toBe("Code block");
    expect(wrapper.find("[data-test='game-room']").exists()).toBe(false);
    const sheet = wrapper.findComponent({ name: "PluginSheet" });
    expect(sheet.props()).toMatchObject({ plugin: TOOL, contact: "c1", sending: "nothing", live: false, text: "# Title", reference: "ref_1", session: "s1", fill: true });
    const back = wrapper.find("[data-test='close-app']");
    expect(back.attributes("aria-label")).toBe("Back");
    await back.trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("opens a game in a room with a way out, its name and an invitation", async () => {
    const wrapper = windowFor(GAME);
    const room = wrapper.find("[data-test='game-room']");
    expect(room.element.tagName).toBe("SECTION");
    expect(room.attributes("aria-label")).toBe("Chess");
    expect(wrapper.find("[data-test='game-bar']").text()).toContain("Chess");
    expect(wrapper.find(".ft-app").exists()).toBe(false);
    expect(wrapper.findComponent({ name: "PluginSheet" }).props()).toMatchObject({ live: true, sending: "propose", fill: false });
    await wrapper.find("[data-test='game-invite']").trigger("click");
    await wrapper.find("[data-test='close-game']").trigger("click");
    expect(wrapper.emitted("invite")).toHaveLength(1);
    expect(wrapper.emitted("close")).toHaveLength(1);
    expect(windowFor({ ...GAME, id: "com.example.game" }).find("[data-test='game-invite']").exists()).toBe(false);
  });

  it("offers no invitation in a game with the built-in FlickerTalk contact", () => {
    store.supportId = "c1";
    try {
      const wrapper = windowFor(GAME);
      expect(wrapper.find("[data-test='game-room']").exists()).toBe(true);
      expect(wrapper.find("[data-test='game-invite']").exists()).toBe(false);
    } finally {
      store.supportId = "";
    }
  });

  it("hides the game while the thread is shown, keeping it running", () => {
    const wrapper = windowFor(GAME, { peeking: true });
    expect(wrapper.find("[data-test='game-area']").attributes("style")).toMatch(/display:\s*none/);
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
  });

  it("passes up what the plugin says", () => {
    const wrapper = windowFor(TOOL);
    const sheet = wrapper.findComponent({ name: "PluginSheet" });
    const need = { key: "location" };
    sheet.vm.$emit("text", "# Title");
    sheet.vm.$emit("attach", { name: "x.pdf" });
    sheet.vm.$emit("openChat", "c2");
    sheet.vm.$emit("done");
    sheet.vm.$emit("closed");
    sheet.vm.$emit("needs", need);
    expect(wrapper.emitted("text")).toEqual([["# Title"]]);
    expect(wrapper.emitted("attach")).toEqual([[{ name: "x.pdf" }]]);
    expect(wrapper.emitted("openChat")).toEqual([["c2"]]);
    expect(wrapper.emitted("done")).toHaveLength(1);
    expect(wrapper.emitted("closed")).toHaveLength(1);
    expect(wrapper.emitted("needs")).toEqual([[need]]);
  });

  it("lets its page reach the plugin to say goodbye, and the game's area", async () => {
    const tool = windowFor(TOOL).vm as unknown as { sheet: { close: () => Promise<void> } | null; gameArea: HTMLElement | null };
    await tool.sheet?.close();
    expect(close).toHaveBeenCalled();
    expect(tool.gameArea).toBeNull();
    const game = windowFor(GAME).vm as unknown as { gameArea: HTMLElement | null };
    expect(game.gameArea?.dataset.test).toBe("game-area");
  });
  describe("full screen", () => {
    afterEach(() => vi.restoreAllMocks());
    const focused = () => vi.spyOn(HTMLElement.prototype, "focus");
    const lastFocused = (spy: ReturnType<typeof focused>) => (spy.mock.contexts.at(-1) as HTMLElement | undefined)?.dataset.test;
    const said = (wrapper: ReturnType<typeof windowFor>) => wrapper.find("[role='status']");

    it("offers it only for a game, as the last button of the room's bar", async () => {
      const wrapper = windowFor(GAME);
      const end = wrapper.findAll("[data-test='game-bar'] ion-buttons[slot='end'] ion-button").map((one) => one.attributes("data-test"));
      expect(end).toEqual(["game-invite", "game-fullscreen"]);
      const expand = wrapper.find("[data-test='game-fullscreen']");
      expect(expand.attributes("aria-label")).toBe("Full screen");
      expect(expand.attributes("aria-pressed")).toBe("false");
      await expand.trigger("click");
      expect(wrapper.emitted("fullscreen")).toEqual([[true]]);

      expect(windowFor({ ...GAME, id: "com.example.game" }).find("[data-test='game-fullscreen']").exists()).toBe(true);
      expect(windowFor(TOOL).find("[data-test='game-fullscreen']").exists()).toBe(false);
    });

    it("covers the window with the same frame, filling it, and gives it back", async () => {
      const wrapper = windowFor(GAME);
      const area = () => wrapper.find("[data-test='game-area']");
      const frame = wrapper.find("iframe").element;
      const sheet = () => wrapper.findComponent({ name: "PluginSheet" });

      await wrapper.setProps({ fullscreen: true });
      expect(area().classes()).toContain("ft-room__game--fullscreen");
      expect(area().element.firstElementChild?.getAttribute("data-test")).toBe("game-fullscreen-bar");
      expect(sheet().props("fill")).toBe(true);
      expect(wrapper.find("[data-test='game-bar']").attributes("inert")).toBeDefined();
      expect(wrapper.find("iframe").element).toBe(frame);

      await wrapper.setProps({ fullscreen: false });
      expect(area().classes()).not.toContain("ft-room__game--fullscreen");
      expect(wrapper.find("[data-test='game-fullscreen-bar']").exists()).toBe(false);
      expect(sheet().props("fill")).toBe(false);
      expect(wrapper.find("[data-test='game-bar']").attributes("inert")).toBeUndefined();
      expect(wrapper.find("iframe").element).toBe(frame);
    });

    it("passes up what its bar asks for", async () => {
      const wrapper = windowFor(GAME, { fullscreen: true });
      expect(wrapper.find("[data-test='game-fullscreen-bar'] .ft-title").text()).toBe("Chess");
      await wrapper.find("[data-test='game-fullscreen-exit']").trigger("click");
      await wrapper.find("[data-test='game-fullscreen-chat']").trigger("click");
      await wrapper.find("[data-test='game-fullscreen-invite']").trigger("click");
      await wrapper.find("[data-test='game-fullscreen-close']").trigger("click");
      expect(wrapper.emitted("fullscreen")).toEqual([[false]]);
      expect(wrapper.emitted("showChat")).toHaveLength(1);
      expect(wrapper.emitted("invite")).toHaveLength(1);
      expect(wrapper.emitted("close")).toHaveLength(1);
      expect(windowFor({ ...GAME, id: "com.example.game" }, { fullscreen: true }).find("[data-test='game-fullscreen-invite']").exists()).toBe(false);
    });

    it("moves the focus to the way back in, and to the expand button when it ends", async () => {
      const wrapper = windowFor(GAME);
      const spy = focused();
      await wrapper.setProps({ fullscreen: true });
      await flushPromises();
      await vi.waitFor(() => expect(lastFocused(spy)).toBe("game-fullscreen-exit"));
      await wrapper.setProps({ fullscreen: false });
      await flushPromises();
      await vi.waitFor(() => expect(lastFocused(spy)).toBe("game-fullscreen"));
    });

    it("leaves the focus alone when it ends because the chat is shown", async () => {
      const wrapper = windowFor(GAME, { fullscreen: true });
      await flushPromises();
      const spy = focused();
      await wrapper.setProps({ fullscreen: false, peeking: true });
      await flushPromises();
      await new Promise((drawn) => requestAnimationFrame(drawn));
      expect(spy).not.toHaveBeenCalled();
    });

    it("says politely that it starts and ends", async () => {
      const wrapper = windowFor(GAME);
      expect(said(wrapper).attributes("aria-live")).toBe("polite");
      await wrapper.setProps({ fullscreen: true });
      await flushPromises();
      expect(said(wrapper).text()).toBe("Full screen");
      await wrapper.setProps({ fullscreen: false });
      await flushPromises();
      expect(said(wrapper).text()).toBe("Exit full screen");
    });

    it("marks the chat and says so when messages arrive", async () => {
      const wrapper = windowFor(GAME, { fullscreen: true });
      await flushPromises();
      expect(wrapper.find("[data-test='game-unread']").exists()).toBe(false);
      await wrapper.setProps({ unread: 2 });
      await flushPromises();
      expect(wrapper.find("[data-test='game-unread']").exists()).toBe(true);
      expect(wrapper.find("[data-test='game-fullscreen-chat']").attributes("aria-label")).toBe("Show the chat, New message");
      expect(said(wrapper).text()).toBe("New message");
    });
  });
});
