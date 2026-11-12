import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { IonButton, IonDatetime, IonSearchbar, IonSegment, IonSegmentButton, IonTextarea } from "@ionic/vue";
import source from "./ChatThread.vue?raw";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import AppTile from "./AppTile.vue";
import { shieldOutline } from "ionicons/icons";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { chat, heardTyping, store, TYPING_EVERY, TYPING_FADE } from "../core";
import { offered, pluginImage, refreshPlugins, premiumLocked } from "../plugins";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";
import { setLocale } from "../i18n";
import { offerSend, takeSend } from "../pending-send";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
const back = vi.hoisted(() => ({ handler: null as null | (() => void), listener: null as null | object }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    const listener = {};
    back.handler = handler;
    back.listener = listener;
    return { unregister: async () => void (back.listener === listener && ((back.handler = null), (back.listener = null))) };
  },
}));
const recorder = vi.hoisted(() => ({
  startRecording: vi.fn(async (): Promise<string> => "recording"),
  stopRecording: vi.fn(async () => new File(["voice"], "voice-20260922-161500.m4a", { type: "audio/mp4" })),
  cancelRecording: vi.fn(),
}));
vi.mock("../recorder", async () => {
  const { reactive } = await import("vue");
  const recording = reactive({ active: false, startedAt: 0 });
  recorder.startRecording.mockImplementation(async () => {
    recording.active = true;
    return "recording";
  });
  recorder.stopRecording.mockImplementation(async () => {
    recording.active = false;
    return new File(["voice"], "voice-20260922-161500.m4a", { type: "audio/mp4" });
  });
  recorder.cancelRecording.mockImplementation(() => {
    recording.active = false;
  });
  return { recording, ...recorder };
});

const IonModalStub = defineComponent({
  name: "IonModal",
  props: { isOpen: Boolean, breakpoints: { type: Array, default: undefined }, initialBreakpoint: { type: Number, default: undefined } },
  emits: ["didDismiss", "didPresent"],
  setup(props, { slots }) {
    return () => (props.isOpen ? h("div", { "data-test": "apps-sheet" }, slots.default?.()) : null);
  },
});
const stubs = { IonIcon: true, IonModal: IonModalStub };

enableAutoUnmount(afterEach);

class FakeViewport extends EventTarget {
  height = 900;
  offsetTop = 0;
  move(height: number) {
    this.height = height;
    this.dispatchEvent(new Event("resize"));
  }
}
const contentWith = (scroller: { scrollHeight: number; clientHeight: number; scrollTop: number }) =>
  defineComponent({
    mounted() {
      (this.$el as { getScrollElement?: () => Promise<unknown> }).getScrollElement = async () => scroller;
    },
    render() {
      return h("div", this.$slots.default?.());
    },
  });

describe("ChatThread", () => {
  beforeEach(async () => {
    seed();
    await refreshPlugins();
    calls.length = 0;
  });

  it("shows every message of the conversation", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.findAllComponents(MessageBubble)).toHaveLength(fixture.chats[0].messages.length);
  });

  describe("day separators", () => {
    afterEach(() => vi.useRealTimers());
    const days = (wrapper: ReturnType<typeof mount>) => wrapper.findAll(".ft-thread__day").map((one) => one.text());

    it("names the day of earlier messages and puts today's under «Today»", async () => {
      vi.useFakeTimers({ toFake: ["Date"] });
      vi.setSystemTime(new Date(2026, 9, 2, 12, 0));
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      chat("c1")?.messages.push({ id: "new", mine: false, text: "still there?", time: "11:58", sentAt: new Date(2026, 9, 2, 11, 58).getTime() });
      await flushPromises();

      expect(days(wrapper)).toEqual(["September 22", "Today"]);
      const [first, today] = wrapper.findAll(".ft-thread__day");
      const bubbles = wrapper.findAllComponents(MessageBubble);
      expect(first.element.nextElementSibling).toBe(bubbles[0].element);
      expect(today.element.nextElementSibling).toBe(bubbles[bubbles.length - 1].element);
    });

    it("shows no separator in an empty thread", async () => {
      store.chats.push({ ...store.chats[0], id: "c9", messages: [] });
      const wrapper = mount(ChatThread, { props: { chatId: "c9" }, shallow: true });
      await flushPromises();

      expect(wrapper.findAllComponents(MessageBubble)).toHaveLength(0);
      expect(days(wrapper)).toEqual([]);
    });
  });

  describe("the support contact", () => {
    afterEach(() => (store.supportId = ""));
    const hint = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='support-hint']");

    it("says in its empty chat that it is automated support", async () => {
      store.chats.push({ ...store.chats[0], id: "ft_support", name: "FlickerTalk", messages: [] });
      store.supportId = "ft_support";
      const wrapper = mount(ChatThread, { props: { chatId: "ft_support" }, shallow: true });
      await flushPromises();
      expect(hint(wrapper).text()).toBe("Automated support. Write and it answers.");
    });

    it("says nothing once the chat has messages, nor in anyone else's empty chat", async () => {
      store.supportId = "c1";
      const withMessages = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      store.chats.push({ ...store.chats[0], id: "c9", messages: [] });
      const someoneElse = mount(ChatThread, { props: { chatId: "c9" }, shallow: true });
      await flushPromises();
      expect(hint(withMessages).exists()).toBe(false);
      expect(hint(someoneElse).exists()).toBe(false);
    });
  });

  it("loads the conversation from the core and marks it read", async () => {
    mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_messages", { contact: "c1", limit: 200 }]);
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
  });

  it("marks newly arrived messages read while the conversation is open", async () => {
    mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    calls.length = 0;
    const shown = chat("c1");
    if (shown) shown.unread = 0;
    shown?.messages.push({ id: "new", mine: false, text: "still there?", time: "09:50", sentAt: Date.now() });
    await flushPromises();
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
  });

  describe("read receipts only for what is on screen", () => {
    let visibility: DocumentVisibilityState = "visible";
    const setVisibility = (state: DocumentVisibilityState) => {
      visibility = state;
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    };
    afterEach(() => Reflect.deleteProperty(document, "visibilityState"));
    const arrive = () => chat("c1")?.messages.push({ id: "new", mine: false, text: "still there?", time: "09:50", sentAt: Date.now() });
    const markedRead = () => calls.some(([command]) => command === "core_mark_read");

    it("does not mark read a message that arrives while the app is in the background", async () => {
      mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      setVisibility("hidden");
      calls.length = 0;
      arrive();
      await flushPromises();
      expect(markedRead()).toBe(false);
    });

    it("marks it read once the app is back on the screen", async () => {
      mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      setVisibility("hidden");
      arrive();
      await flushPromises();
      calls.length = 0;
      setVisibility("visible");
      document.dispatchEvent(new Event("visibilitychange"));
      await flushPromises();
      expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
    });

    it("does not mark the conversation read when it opens with the app in the background", async () => {
      setVisibility("hidden");
      mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      expect(markedRead()).toBe(false);
    });

    it("waits while another page covers the conversation and marks read once it is back", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", active: true }, shallow: true });
      await flushPromises();
      await wrapper.setProps({ active: false });
      calls.length = 0;
      arrive();
      await flushPromises();
      expect(markedRead()).toBe(false);

      await wrapper.setProps({ active: true });
      await flushPromises();
      expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
    });
  });

  it("sends what is written and clears the composer", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "  hello there ");
    await flushPromises();
    await wrapper.find("[aria-label='Send']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_send", { contact: "c1", text: "hello there" }]);
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
  });

  it("sends the files picked with the attach button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    const input = wrapper.find("input[type='file']");
    const file = new File(["menu"], "menu.pdf", { type: "application/pdf" });
    Object.defineProperty(input.element, "files", { value: [file] });
    await input.trigger("change");
    await flushPromises();
    expect(calls).toContainEqual(["core_send_file", { contact: "c1", upload: "up1", name: "menu.pdf", mime: "application/pdf" }]);
  });

  it("sends again through the core a message that was not sent", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    wrapper.findAllComponents(MessageBubble)[0].vm.$emit("resend", "m3");
    await flushPromises();
    expect(calls).toContainEqual(["core_resend", { message: "m3" }]);
  });

  it("opens and saves files through the core", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    const bubble = wrapper.findAllComponents(MessageBubble)[0];
    bubble.vm.$emit("open", "m4");
    bubble.vm.$emit("save", "m4");
    await flushPromises();
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
    expect(calls).toContainEqual(["core_save_file", { message: "m4" }]);
    expect(wrapper.findAllComponents(MessageBubble).find((b) => b.props("message").id === "m4")?.props("saved")).toBe(true);
  });

  it("records and sends a voice message", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Record voice message']").trigger("click");
    await flushPromises();
    expect(recorder.startRecording).toHaveBeenCalled();
    expect(wrapper.find("[data-test='recording']").exists()).toBe(true);
    await wrapper.find("[aria-label='Send voice message']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual([
      "core_send_file",
      { contact: "c1", upload: "up1", name: "voice-20260922-161500.m4a", mime: "audio/mp4" },
    ]);
    expect(wrapper.find("[data-test='recording']").exists()).toBe(false);
  });

  it("says when it cannot record", async () => {
    recorder.startRecording.mockImplementationOnce(async () => "unsupported");
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Record voice message']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='recording']").exists()).toBe(false);
    expect(wrapper.find("[role='alert']").text()).toContain("Can't record here");
    recorder.startRecording.mockImplementationOnce(async () => "denied");
    await wrapper.find("[aria-label='Record voice message']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toContain("microphone");
  });

  it("throws a voice message away", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Record voice message']").trigger("click");
    await flushPromises();
    await wrapper.find("[aria-label='Discard voice message']").trigger("click");
    expect(recorder.cancelRecording).toHaveBeenCalled();
    expect(calls.some(([command]) => command === "core_send_file")).toBe(false);
  });

  it("offers voice and video calls", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Voice call']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Video call']").exists()).toBe(true);
  });

  describe("typing", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("says the contact is typing until their message comes, or for a moment", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      expect(wrapper.find("[data-test='peer-status']").text()).toBe("Direct");
      heardTyping("c1");
      await wrapper.vm.$nextTick();
      expect(wrapper.find("[data-test='peer-status']").text()).toBe("typing…");
      expect(wrapper.find("[data-test='peer-status']").classes()).toContain("is-typing");
      expect(wrapper.find("[data-test='peer']").attributes("aria-label")).toBe("Contact details: Maria López, typing…");
      vi.advanceTimersByTime(TYPING_FADE + 1);
      await wrapper.vm.$nextTick();
      expect(wrapper.find("[data-test='peer-status']").text()).toBe("Direct");
    });

    it("tells the core the user is writing, once in a while and not for every key", async () => {
      vi.advanceTimersByTime(TYPING_EVERY);
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      const typed = () => calls.filter(([command]) => command === "core_typing");
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "h");
      await flushPromises();
      expect(typed()).toEqual([["core_typing", { contact: "c1" }]]);
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "he");
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hel");
      await flushPromises();
      expect(typed()).toHaveLength(1);
      vi.advanceTimersByTime(TYPING_EVERY + 1);
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hell");
      await flushPromises();
      expect(typed()).toHaveLength(2);
    });

    it("says nothing while the composer is emptied", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "   ");
      await flushPromises();
      expect(calls.filter(([command]) => command === "core_typing")).toEqual([]);
    });
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

  it("names every button in the header", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1", showBack: true }, shallow: true });
    const buttons = wrapper.find(".ft-thread__bar").findAll("button, ion-button-stub, ion-back-button-stub");
    expect(buttons.length).toBeGreaterThanOrEqual(4);
    for (const button of buttons) expect(button.attributes("aria-label"), button.html()).toBeTruthy();
    expect(wrapper.find("[data-test='peer']").attributes("aria-label")).toBe("Contact details: Maria López, Direct");
  });

  it("opens the contact details from the header", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[data-test='peer']").trigger("click");
    expect(push).toHaveBeenCalledWith("/contact/c1");
  });

  it("has a composer to attach, record and send", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Attach']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(true);
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hi");
    await flushPromises();
    expect(wrapper.find("[aria-label='Send']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(false);
  });

  it("unfolds photo-or-video and file from the attach button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Attach photo or video']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_pick_files", { accept: "image/*,video/*" }]);
    await wrapper.find("[aria-label='Attach file']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_pick_files", { accept: "" }]);
  });

  it("takes a photo with the camera from the attach button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Take a photo']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_take_photo", {}]);
    expect(calls).toContainEqual([
      "core_send_picked",
      { contact: "c1", file: { path: "/data/uploads/photo.jpg", name: "photo-20260923-201530.jpg", mime: "image/jpeg", size: 1234 } },
    ]);
  });

  it("says so when no photo can be taken", async () => {
    const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    const answer = internals.invoke;
    internals.invoke = (command, args) => (command === "core_take_photo" ? Promise.reject(new Error("no camera on this device")) : answer(command, args));
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Take a photo']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toBe("Can't take a photo: the camera is not available or not allowed");
  });

  it("opens a plugin from the apps button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(false);

    await wrapper.find("[data-test='apps']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='apps-sheet']").html()).toContain("Code block");

    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
    expect(wrapper.findComponent({ name: "PluginSheet" }).props("fill")).toBe(true);
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(true);
    expect(wrapper.find(".ft-app__bar .ft-title").text()).toBe("Code block");
    const close = wrapper.find("[data-test='close-app']");
    expect(close.element.tagName).toBe("ION-BUTTON");
    expect(close.element.closest("ion-toolbar")?.getAttribute("data-test")).toBe("app-bar");
    expect(close.element.closest("ion-buttons")?.getAttribute("slot")).toBe("start");
    expect(close.attributes("aria-label")).toBe("Back");
    expect(wrapper.find("[data-test='app-bar'] ion-title").exists()).toBe(true);
  });

  it("reads the installed plugins when the apps sheet opens, not when the chat opens", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(calls.some(([command]) => command === "core_plugins")).toBe(false);

    await wrapper.find("[data-test='apps']").trigger("click");
    await flushPromises();
    expect(calls.filter(([command]) => command === "core_plugins")).toHaveLength(1);
    expect(calls.some(([command]) => command === "core_plan")).toBe(true);
  });

  it("opens a plugin in the hidden session the conversation lives in", async () => {
    const open = async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='apps']").trigger("click");
      await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
      await flushPromises();
      return wrapper.findComponent({ name: "PluginSheet" }).props("session");
    };
    expect(await open()).toBeUndefined();
    const c1 = store.chats.find((one) => one.id === "c1")!;
    store.sessions = [{ id: "s1", chats: [c1], requests: [], circles: [] }];
    store.chats = store.chats.filter((one) => one.id !== "c1");
    expect(await open()).toBe("s1");
  });

  it("notices a tool added or removed while the conversation stays open", async () => {
    const removeAll = async () => {
      installTauri((command, args) => {
        calls.push([command, args]);
        return command === "core_plugins" ? [] : undefined;
      });
      await refreshPlugins();
      await flushPromises();
    };
    const open = async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      return wrapper;
    };

    const android = await open();
    await android.find("[data-test='apps']").trigger("click");
    expect(android.find("[data-test='app-com.flickertalk.code']").exists()).toBe(true);
    await removeAll();
    expect(android.find("[data-test='app-com.flickertalk.code']").exists()).toBe(false);
    expect(android.find("[data-test='apps']").exists()).toBe(true);

    seed();
    vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)");
    const iphone = await open();
    expect(iphone.find("[data-test='apps']").exists()).toBe(true);
    await removeAll();
    expect(iphone.find("[data-test='apps']").exists()).toBe(true);
    vi.restoreAllMocks();
  });

  it("puts in the composer the text a plugin proposes", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await wrapper.find("[data-test='apps']").trigger("click");
    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();

    wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("text", "# Title");
    await flushPromises();
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("# Title");
  });

  it("stages what a plugin made and sends it only when the user says so", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await wrapper.find("[data-test='apps']").trigger("click");
    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();
    const sheet = wrapper.findComponent({ name: "PluginSheet" });
    expect(sheet.props("sending")).toBe("nothing");

    const staged = { path: "/data/files/outgoing/1-clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 };
    sheet.vm.$emit("attach", staged);
    await flushPromises();
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(wrapper.find("[data-test='staged']").text()).toContain("clean.jpg");
    expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);

    await wrapper.find("[data-test='staged-send']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_send_picked", { contact: "c1", file: staged }]);
    expect(wrapper.find("[data-test='staged']").exists()).toBe(false);
  });

  describe("a proposal from a tool opened on its own", () => {
    it("puts a text in the composer once the conversation is on screen, once", async () => {
      offerSend("c1", { kind: "text", text: "# Title" });
      const wrapper = mount(ChatThread, { props: { chatId: "c1", active: false }, shallow: false, global: { stubs } });
      await flushPromises();
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
      await wrapper.setProps({ active: true });
      await flushPromises();
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("# Title");
      expect(takeSend("c1")).toBeNull();
      expect(calls.some(([command]) => command === "core_send")).toBe(false);
    });

    it("stages a file for the user to send", async () => {
      const staged = { path: "/data/files/outgoing/1-notes.md", name: "notes.md", mime: "text/markdown", size: 4 };
      offerSend("c1", { kind: "file", file: staged });
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      expect(wrapper.find("[data-test='staged']").text()).toContain("notes.md");
      expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);
      await wrapper.find("[data-test='staged-send']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_send_picked", { contact: "c1", file: staged }]);
    });

    it("leaves a proposal for another conversation where it is", async () => {
      offerSend("c2", { kind: "text", text: "for Alex" });
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("");
      expect(takeSend("c2")).toEqual({ kind: "text", text: "for Alex" });
    });
  });

  it("throws away what a plugin made if the user does not want it", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await wrapper.find("[data-test='apps']").trigger("click");
    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();
    wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("attach", { path: "/p", name: "x.pdf", mime: "application/pdf", size: 1 });
    await flushPromises();
    await wrapper.find("[data-test='staged-discard']").trigger("click");
    expect(wrapper.find("[data-test='staged']").exists()).toBe(false);
    expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);
  });

  it("asks for a waiting file when its bubble says so", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    wrapper.findAllComponents(MessageBubble)[0].vm.$emit("download", "m1");
    await flushPromises();
    expect(calls).toContainEqual(["core_accept_file", { message: "m1" }]);
  });

  it("puts the emoji that was picked at the end of what is being written", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(wrapper.findComponent({ name: "EmojiPicker" }).exists()).toBe(false);

    await wrapper.find("[data-test='open-emoji']").trigger("click");
    const picker = wrapper.findComponent({ name: "EmojiPicker" });
    expect(picker.exists()).toBe(true);

    picker.vm.$emit("pick", "🎉");
    await flushPromises();
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("🎉");
  });

  it("closes the emoji when the message goes", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await wrapper.find("[data-test='open-emoji']").trigger("click");
    wrapper.findComponent({ name: "EmojiPicker" }).vm.$emit("pick", "🎉");
    await flushPromises();

    await wrapper.find("[aria-label='Send']").trigger("click");
    await flushPromises();
    expect(wrapper.findComponent({ name: "EmojiPicker" }).exists()).toBe(false);
    expect(calls).toContainEqual(["core_send", { contact: "c1", text: "🎉" }]);
  });

  async function pressed(wrapper: ReturnType<typeof mount>) {
    await wrapper.findAll("[data-test='bubble']")[0].trigger("pointerdown");
    await new Promise((wake) => setTimeout(wake, 550));
    await flushPromises();
    return wrapper;
  }

  it("offers four things to do with a message, on a long press", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);

    await pressed(wrapper);
    const actions = wrapper.find("[data-test='actions']");
    expect(actions.exists()).toBe(true);
    for (const what of ["fold", "forward", "share", "delete"]) {
      expect(actions.find(`[data-test='${what}']`).exists()).toBe(true);
    }
  });

  it("opens a message with a plugin that says it opens its kind", async () => {
    const bridge = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
    const fallback = bridge.invoke;
    bridge.invoke = (command, args) => {
      if (command === "core_plugins") {
        return Promise.resolve([
          {
            id: "com.flickertalk.notes",
            name: "Notes",
            version: "1.0.0",
            asks: { network: [], messages: true, send: "nothing" },
            granted: { network: [], messages: true, send: "nothing" },
            installedAt: 1,
            opens: ["text/plain"],
          },
        ]);
      }
      if (command === "core_plugin_ref") {
        calls.push([command, args]);
        return Promise.resolve("ref_1");
      }
      return fallback(command, args);
    };
    await refreshPlugins();
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    await wrapper.find("[data-test='open-with']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='open-with-com.flickertalk.notes']").text()).toBe("Notes");
    await wrapper.find("[data-test='open-with-com.flickertalk.notes']").trigger("click");
    await flushPromises();
    const sheet = wrapper.findComponent({ name: "PluginSheet" });
    expect(sheet.exists()).toBe(true);
    expect(sheet.props("text")).toBe(fixture.chats[0].messages[0].text);
    expect(sheet.props("reference")).toBe("ref_1");
    expect(calls).toContainEqual(["core_plugin_ref", { plugin: "com.flickertalk.notes", message: "m1" }]);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
  });

  describe("open with a tool not yet allowed to read the message", () => {
    const TRANSLATOR = {
      id: "com.flickertalk.translate",
      name: "Translator",
      version: "1.0.0",
      asks: { network: [], messages: true, send: "propose", translate: true },
      granted: { network: [], messages: false, send: "nothing", translate: false },
      installedAt: 1,
      opens: ["text/plain"],
    };

    const NOTES = {
      id: "com.flickertalk.notes",
      name: "Notes",
      version: "1.0.0",
      asks: { network: [], messages: true, send: "nothing" },
      granted: { network: [], messages: true, send: "nothing" },
      installedAt: 1,
      opens: ["text/plain"],
    };
    const MARKDOWN = {
      id: "com.flickertalk.markdown",
      name: "Markdown",
      version: "1.0.0",
      asks: { network: [], messages: false, send: "propose" },
      granted: { network: [], messages: false, send: "nothing" },
      installedAt: 1,
    };
    const BLIND = { ...NOTES, id: "com.example.blind", name: "Blind", asks: { ...NOTES.asks, messages: false }, granted: { ...NOTES.granted, messages: false } };

    async function withTools(...others: object[]) {
      const bridge = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const fallback = bridge.invoke;
      let views: Array<Record<string, unknown>> = JSON.parse(JSON.stringify([TRANSLATOR, ...others]));
      bridge.invoke = (command, args) => {
        if (command === "core_plugins") return Promise.resolve(JSON.parse(JSON.stringify(views)));
        if (command === "core_plugin_grant") {
          calls.push([command, args]);
          views = views.map((one) => (one.id === args?.plugin ? { ...one, granted: args?.granted } : one));
          return Promise.resolve(undefined);
        }
        if (command === "core_plugin_ref") return Promise.resolve("ref_1");
        return fallback(command, args);
      };
      await refreshPlugins();
    }

    async function openWithList(wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } })) {
      await flushPromises();
      await pressed(wrapper);
      await wrapper.find("[data-test='open-with']").trigger("click");
      await flushPromises();
      return wrapper;
    }

    async function chooseTranslator() {
      await withTools();
      const wrapper = await openWithList();
      expect(wrapper.find("[data-test='open-with-com.flickertalk.translate']").text()).toBe("Translator");
      await wrapper.find("[data-test='open-with-com.flickertalk.translate']").trigger("click");
      await flushPromises();
      return wrapper;
    }
    const asks = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='permission-ask']");
    const grants = () => calls.filter(([command]) => command === "core_plugin_grant");

    it("asks for reading the message, and opens the tool with its text after a yes", async () => {
      const wrapper = await chooseTranslator();
      expect(asks(wrapper).html()).toContain("Translator needs a permission");
      expect(asks(wrapper).html()).toContain("Read what you send it");
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
      expect(grants()).toHaveLength(0);

      await wrapper.find("[data-test='permission-allow']").trigger("click");
      await flushPromises();
      expect(grants()).toEqual([["core_plugin_grant", { plugin: TRANSLATOR.id, granted: expect.objectContaining({ messages: true, translate: false, send: "nothing" }) }]]);
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.exists()).toBe(true);
      expect(sheet.props("text")).toBe(fixture.chats[0].messages[0].text);
      expect(sheet.props("reference")).toBe("ref_1");
      expect(asks(wrapper).exists()).toBe(false);
    });

    it("opens nothing and grants nothing when the user says no", async () => {
      const wrapper = await chooseTranslator();
      await wrapper.find("[data-test='permission-cancel']").trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
      expect(grants()).toHaveLength(0);
      expect(asks(wrapper).exists()).toBe(false);
    });

    it("is listed with the tools already allowed, never with one that opens no text or reads none", async () => {
      await withTools(NOTES, MARKDOWN, BLIND);
      const wrapper = await openWithList();
      const listed = wrapper.findAll("[data-test^='open-with-']").map((one) => one.attributes("data-test"));
      expect(listed).toEqual(["open-with-com.flickertalk.notes", "open-with-com.flickertalk.translate"]);
    });

    it("asks once: read again from the core, the tool opens at once the next time", async () => {
      const wrapper = await chooseTranslator();
      await wrapper.find("[data-test='permission-allow']").trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
      await wrapper.find("[data-test='close-app']").trigger("click");
      await flushPromises();

      await refreshPlugins();
      await openWithList(wrapper);
      await wrapper.find("[data-test='open-with-com.flickertalk.translate']").trigger("click");
      await flushPromises();
      expect(asks(wrapper).exists()).toBe(false);
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.exists()).toBe(true);
      expect(sheet.props("text")).toBe(fixture.chats[0].messages[0].text);
      expect(sheet.props("reference")).toBe("ref_1");
      expect(grants()).toHaveLength(1);
    });

    it("asks again the next time after a no", async () => {
      const wrapper = await chooseTranslator();
      await wrapper.find("[data-test='permission-cancel']").trigger("click");
      await flushPromises();

      await openWithList(wrapper);
      await wrapper.find("[data-test='open-with-com.flickertalk.translate']").trigger("click");
      await flushPromises();
      expect(asks(wrapper).exists()).toBe(true);
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
      expect(grants()).toHaveLength(0);
    });
  });

  describe("the chat's hooks", () => {
    const ID = "com.flickertalk.translate";
    const KEY = "k".repeat(43);
    const TRANSLATOR = {
      id: ID,
      name: "Translator",
      version: "1.1.0",
      kind: "tool",
      asks: { network: [], messages: true, send: "propose", translate: true },
      granted: { network: [], messages: true, send: "propose", translate: true },
      installedAt: 1,
      hooks: {
        message: { label: "Translate", icon: "language-outline" },
        composer: { label: "Translate", icon: "language-outline" },
        settings: { label: "Translation", icon: "language-outline", fields: [{ key: "lang", type: "select", label: "Language" }] },
      },
    };
    type Hook = { name: string; text: string; chat: string; ref: string | null; lang: string; manual: boolean; answer?: boolean };
    let answer: (hook: Hook) => unknown = () => null;
    const sheets: { props: Record<string, unknown>; runHook: ReturnType<typeof vi.fn> }[] = [];
    const HookSheet = defineComponent({
      name: "PluginSheet",
      props: {
        plugin: { type: Object, default: undefined },
        contact: { type: String, default: "" },
        hidden: Boolean,
        hook: { type: String, default: undefined },
        text: { type: String, default: undefined },
        sending: { type: String, default: "nothing" },
        live: Boolean,
        session: { type: String, default: undefined },
        fill: Boolean,
        file: { type: Object, default: undefined },
        reference: { type: String, default: undefined },
      },
      emits: ["text", "attach", "done", "closed", "needs", "openChat", "shown", "refused", "pick", "sendTo"],
      setup(props, { expose, emit }) {
        const runHook = vi.fn(async (hook: Hook) => answer(hook));
        sheets.push({ props, runHook });
        expose({ runHook, close: async () => emit("closed") });
        return () => h("div", { "data-test": props.hidden ? "hook-frame" : "plugin-window" });
      },
    });
    const hookStubs = { ...stubs, PluginSheet: HookSheet };
    let view: Record<string, unknown> = TRANSLATOR;
    let flags: unknown[] = [];
    let sentId: string | undefined;
    let sample = "";

    beforeEach(() => {
      sheets.length = 0;
      answer = () => null;
      view = JSON.parse(JSON.stringify(TRANSLATOR));
      flags = [];
      sentId = undefined;
      sample = "";
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const fallback = internals.invoke;
      internals.invoke = (command, args) => {
        if (command === "core_plugins") return Promise.resolve([view]);
        if (command === "core_chat_hooks") return Promise.resolve(flags);
        if (command === "core_plugin_chat") return Promise.resolve(KEY);
        if (command === "core_plugin_ref") return Promise.resolve("ref_1");
        if (command === "core_hook_sample") return Promise.resolve(sample);
        if (command === "core_plugin_grant") view = { ...view, granted: args?.granted };
        if (command === "core_send") {
          calls.push([command, args]);
          return Promise.resolve(sentId);
        }
        return fallback(command, args);
      };
    });
    afterEach(() => {
      premiumLocked.value = false;
    });

    async function thread() {
      await refreshPlugins();
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: hookStubs } });
      await flushPromises();
      return wrapper;
    }
    const kept = (command: string) => calls.filter(([one]) => one === command).map(([, args]) => args);
    const frames = () => sheets.filter((one) => one.props.hidden);
    const offering = (wrapper: ReturnType<typeof mount>) =>
      wrapper.findAllComponents(MessageBubble).filter((one) => one.find("[data-test='translate']").exists()).map((one) => one.props("message").id);

    it("keeps the header, the long press and the composer as they were: no hook buttons there", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: false, lang: "fr" }];
      const wrapper = await thread();
      expect(wrapper.find("ion-header [data-test^='chat-hook-']").exists()).toBe(false);
      await pressed(wrapper);
      expect(wrapper.find("[data-test='actions']").exists()).toBe(true);
      expect(wrapper.find("[data-test^='hook-message-']").exists()).toBe(false);
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hello");
      await flushPromises();
      expect(wrapper.find("[data-test^='hook-composer-']").exists()).toBe(false);
      expect(wrapper.find("[data-test='your-original']").exists()).toBe(false);
      expect(wrapper.find("[data-test='translating']").exists(), "nothing runs by itself").toBe(false);
    });

    it("offers the hook's link under each received text while the conversation's language is not known", async () => {
      const wrapper = await thread();
      expect(offering(wrapper)).toEqual(["m1", "m3", "m6"]);
    });

    it("offers it while the conversation is in another language, and not in the app's", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: false, lang: "fr" }];
      expect(offering(await thread())).toEqual(["m1", "m3", "m6"]);
      flags = [{ plugin: ID, autoMessage: false, autoComposer: false, lang: "en" }];
      expect(offering(await thread())).toEqual([]);
    });

    it("offers nothing without a tool bound to messages", async () => {
      view = { ...TRANSLATOR, hooks: { composer: TRANSLATOR.hooks.composer } };
      expect(offering(await thread())).toEqual([]);
    });

    it("runs the message hook out of sight on a tap, and keeps its answer for the bubble", async () => {
      answer = (hook) => (hook.name === "message" ? { text: "¿Vienes esta noche?" } : null);
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(frames()).toHaveLength(1);
      expect(frames()[0].runHook).toHaveBeenCalledWith({ name: "message", text: "Are you coming tonight?", chat: KEY, ref: "ref_1", lang: "en", manual: true });
      expect(kept("core_message_note_set")).toEqual([{ message: "m1", plugin: ID, text: "¿Vienes esta noche?" }]);
    });

    it("asks first when the tool may not read the message yet, and runs only after a yes", async () => {
      view = { ...TRANSLATOR, granted: { ...TRANSLATOR.granted, messages: false } };
      answer = () => ({ text: "hola" });
      const wrapper = await thread();
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='permission-ask']").html()).toContain("Read what you send it");
      expect(frames()).toHaveLength(0);
      await wrapper.find("[data-test='permission-allow']").trigger("click");
      await flushPromises();
      expect(frames()[0].runHook).toHaveBeenCalledWith(expect.objectContaining({ name: "message", manual: true }));
      expect(kept("core_message_note_set")).toHaveLength(1);
    });

    it("leads a locked tool's link to Premium", async () => {
      const wrapper = await thread();
      premiumLocked.value = true;
      await wrapper.findAllComponents(MessageBubble)[0].find("[data-test='translate']").trigger("click");
      await flushPromises();
      expect(push).toHaveBeenCalledWith("/tabs/settings#premium");
      expect(frames()).toHaveLength(0);
    });

    it("says under the header that it translates by itself, into what, and leads to the contact's page", async () => {
      flags = [{ plugin: ID, autoMessage: true, autoComposer: false, lang: "fr" }];
      const wrapper = await thread();
      const chip = wrapper.find("[data-test='translating']");
      expect(chip.html()).toContain("Translating · French");
      await chip.trigger("click");
      expect(push).toHaveBeenCalledWith("/contact/c1");
    });

    it("sends in the conversation's language with its composer switch on", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      sentId = "sent1";
      answer = (hook) => (hook.name === "composer" ? { text: "bonjour" } : null);
      const wrapper = await thread();
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hello");
      await flushPromises();
      await wrapper.find("[aria-label='Send']").trigger("click");
      await flushPromises();
      expect(frames()[0].runHook).toHaveBeenCalledWith(expect.objectContaining({ name: "composer", text: "hello", manual: false }));
      expect(kept("core_send")).toEqual([{ contact: "c1", text: "bonjour" }]);
    });

    it("keeps the user's words under the message sent in the conversation's language", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      sentId = "sent1";
      answer = (hook) => (hook.name === "composer" ? { text: "bonjour" } : null);
      const wrapper = await thread();
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hello");
      await flushPromises();
      await wrapper.find("[aria-label='Send']").trigger("click");
      await flushPromises();
      expect(kept("core_message_note_set")).toEqual([{ message: "sent1", plugin: ID, text: "hello" }]);
    });

    it("keeps nothing under a text the tool sent unchanged, or could not translate", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      sentId = "sent1";
      const answers: unknown[] = [{ text: "ok" }, null];
      answer = (hook) => (hook.name === "composer" ? answers.shift() : null);
      const wrapper = await thread();
      for (const words of ["ok", "hello"]) {
        wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", words);
        await flushPromises();
        await wrapper.find("[aria-label='Send']").trigger("click");
        await flushPromises();
      }
      expect(kept("core_send")).toEqual([
        { contact: "c1", text: "ok" },
        { contact: "c1", text: "hello" },
      ]);
      expect(kept("core_message_note_set")).toEqual([]);
    });

    it("names the conversation's language on my texts sent in it", async () => {
      flags = [{ plugin: ID, autoMessage: false, autoComposer: true, lang: "fr" }];
      const wrapper = await thread();
      expect(wrapper.findAllComponents(MessageBubble)[1].props("language")).toBe("French");
    });

    it("looks for the language of a conversation without one as it opens, and keeps it", async () => {
      sample = "Are you coming tonight?\nSee you at 6 then ✨";
      answer = (hook) => (hook.name === "settings" ? { lang: "it" } : null);
      await thread();
      await vi.waitFor(() => expect(kept("core_chat_hooks_set")).toEqual([{ conversation: "c1", plugin: ID, autoMessage: false, autoComposer: false, lang: "it" }]));
      expect(frames()[0].runHook).toHaveBeenCalledWith({ name: "settings", text: sample, chat: KEY, ref: null, lang: "en", manual: false });
    });

    it("translates the texts received by itself with the conversation's message switch on, the newest first", async () => {
      flags = [{ plugin: ID, autoMessage: true, autoComposer: false, lang: "fr" }];
      answer = (hook) => ({ text: `[${hook.text}]` });
      await thread();
      await vi.waitFor(() => expect(kept("core_message_note_set")).toHaveLength(3));
      expect(frames()[0].runHook.mock.calls.map(([hook]) => [hook.text, hook.manual])).toEqual([
        ["See you at 6 then ✨", false],
        ["Perfect, I'll book the table 🍝", false],
        ["Are you coming tonight?", false],
      ]);
    });
  });

  it("names a tool in the phone's language wherever it is shown", async () => {
    await setLocale("es");
    try {
      const bridge = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const fallback = bridge.invoke;
      bridge.invoke = (command, args) => {
        if (command === "core_plugins") {
          return Promise.resolve([
            {
              id: "com.flickertalk.notes",
              name: "Notes",
              version: "1.0.0",
              asks: { network: [], messages: true, send: "nothing" },
              granted: { network: [], messages: true, send: "nothing" },
              installedAt: 1,
              opens: ["text/plain"],
              locales: { es: { name: "Notas" } },
            },
          ]);
        }
        if (command === "core_plugin_ref") return Promise.resolve("ref_1");
        return fallback(command, args);
      };
      await refreshPlugins();
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='apps']").trigger("click");
      expect(wrapper.find("[data-test='app-com.flickertalk.notes'] .ft-app-tile__name").text()).toBe("Notas");
      await wrapper.find("[data-test='app-com.flickertalk.notes']").trigger("click");
      await flushPromises();
      expect(wrapper.find(".ft-app__bar .ft-title").text()).toBe("Notas");
      expect(wrapper.find(".ft-app").attributes("aria-label")).toBe("Notas");
      await wrapper.find("[data-test='close-app']").trigger("click");

      await pressed(wrapper);
      await wrapper.find("[data-test='open-with']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='open-with-com.flickertalk.notes']").text()).toBe("Notas");
    } finally {
      await setLocale("en");
    }
  });

  function withPlugins(plugins: unknown[], answers: Record<string, unknown> = {}) {
    const bridge = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> } }).__TAURI_INTERNALS__;
    const fallback = bridge.invoke;
    bridge.invoke = (command, args) => {
      if (command === "core_plugins") return Promise.resolve(plugins);
      if (command in answers) {
        calls.push([command, args]);
        const answer = answers[command];
        return answer instanceof Error ? Promise.reject(answer) : Promise.resolve(answer);
      }
      return fallback(command, args);
    };
    void refreshPlugins();
  }
  const VIEWER = {
    id: "com.flickertalk.pdfviewer",
    name: "PDF viewer",
    version: "1.0.0",
    asks: { network: [], messages: false, send: "nothing" },
    granted: { network: [], messages: false, send: "nothing" },
    installedAt: 2,
    opens: ["application/pdf"],
    views: ["application/pdf"],
  };
  const DRIVE = { ...VIEWER, id: "com.flickertalk.drive", name: "My drive", opens: ["*/*"], views: [] };
  const tapped = async (wrapper: ReturnType<typeof mount>, id: string) => {
    const bubble = wrapper.findAllComponents(MessageBubble).find((one) => one.props("message").id === id);
    bubble?.vm.$emit("open", id);
    await flushPromises();
  };

  it("shows a tapped file in its viewer, with the bytes and the way back", async () => {
    withPlugins([DRIVE, VIEWER], { core_read_message_file: { name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" }, core_plugin_ref: "ref_4" });
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await tapped(wrapper, "m4");
    const sheet = wrapper.findComponent({ name: "PluginSheet" });
    expect(sheet.exists()).toBe(true);
    expect(sheet.props("plugin").id).toBe("com.flickertalk.pdfviewer");
    expect(sheet.props("file")).toEqual({ name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" });
    expect(sheet.props("reference")).toBe("ref_4");
    expect(calls.some(([command]) => command === "core_open_file")).toBe(false);
  });

  it("sends a tapped file to another app when there is no viewer, or the bytes cannot be handed over", async () => {
    withPlugins([DRIVE], {});
    let wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await tapped(wrapper, "m4");
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);

    calls.length = 0;
    withPlugins([VIEWER], { core_read_message_file: new Error("that file is too big for a plugin") });
    wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await tapped(wrapper, "m4");
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
  });

  describe("with the tools locked", () => {
    const limited = (plugins: unknown[], answers: Record<string, unknown> = {}) => withPlugins(plugins, { core_plan: { state: "limited", until: 0 }, ...answers });
    afterEach(() => (premiumLocked.value = false));

    it("shows a lock on each tool of the apps sheet and goes to the Premium section of Settings", async () => {
      push.mockClear();
      limited([VIEWER]);
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='apps']").trigger("click");
      await flushPromises();
      const row = wrapper.find(`[data-test='app-${VIEWER.id}']`);
      expect(wrapper.findAllComponents(AppTile).find((one) => one.attributes("data-test") === `app-${VIEWER.id}`)?.props("badge")).toBe("lock");
      expect(row.attributes("aria-label")).toContain("locked");
      await row.trigger("click");
      await flushPromises();
      expect(push).toHaveBeenCalledWith("/tabs/settings#premium");
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    });

    it("sends a tapped file to another app rather than to a locked viewer", async () => {
      limited([VIEWER], { core_read_message_file: { name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" } });
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await tapped(wrapper, "m4");
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
      expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
    });
  });

  describe("with a contact who is blocked", () => {
    beforeEach(() => {
      store.chats[0].blocked = true;
    });

    it("shows a notice and Unblock instead of the composer", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      const panel = wrapper.find("[data-test='blocked-panel']");
      expect(panel.exists()).toBe(true);
      expect(panel.text()).toContain("You blocked this contact");
      expect(wrapper.findComponent(IonTextarea).exists()).toBe(false);
    });

    it("unblocks them from the notice", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='unblock']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_block", { contact: "c1", blocked: false }]);
    });
  });

  describe("with someone who wrote first", () => {
    beforeEach(() => {
      store.requests = [{ ...store.chats[1], id: "ft_stranger", name: "Mamá", unread: 1, preview: "hi" }];
    });

    it("shows who it is instead of the composer, and no way to call yet", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs } });
      await flushPromises();
      const panel = wrapper.find("[data-test='request-panel']");
      expect(panel.exists()).toBe(true);
      expect(panel.text()).toContain("Mamá");
      expect(wrapper.findComponent(IonTextarea).exists()).toBe(false);
      expect(wrapper.find(`[aria-label='Voice call']`).exists()).toBe(false);
      expect(wrapper.find(`[aria-label='Video call']`).exists()).toBe(false);
      expect(wrapper.find("[data-test='apps']").exists()).toBe(false);
    });

    it("blocks only after asking once", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='request-decline']").trigger("click");
      expect(calls.some(([command]) => command === "core_decline_contact")).toBe(false);
      expect(wrapper.find("[data-test='request-decline-ask']").text()).toContain("Mamá");
      await wrapper.find("[data-test='request-decline-cancel']").trigger("click");
      expect(wrapper.find("[data-test='request-decline-ask']").exists()).toBe(false);

      await wrapper.find("[data-test='request-decline']").trigger("click");
      await wrapper.find("[data-test='request-decline-confirm']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_decline_contact", { contact: "ft_stranger" }]);
      expect(push).toHaveBeenCalledWith("/tabs/chats");
    });

    it("accepts, and then the conversation is like any other", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs } });
      await flushPromises();
      const stranger = { ...store.requests[0] };
      await wrapper.find("[data-test='request-accept']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_accept_contact", { contact: "ft_stranger" }]);
      store.chats.push(stranger);
      store.requests = [];
      await flushPromises();
      expect(wrapper.find("[data-test='request-panel']").exists()).toBe(false);
      expect(wrapper.findComponent(IonTextarea).exists()).toBe(true);
    });
  });

  it("closes the plugin, then the actions, with the back button, and stays in the chat", async () => {
    withPlugins([VIEWER], { core_read_message_file: { name: "menu.pdf", mime: "application/pdf", data: "JVBERi0=" }, core_plugin_ref: "ref_4" });
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(back.handler).toBeNull();

    await tapped(wrapper, "m4");
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
    back.handler?.();
    await flushPromises();
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(back.handler).toBeNull();

    await pressed(wrapper);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(true);
    back.handler?.();
    await flushPromises();
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
    expect(push).not.toHaveBeenCalled();
  });

  it("offers another app from «open with» only when a viewer takes the tap", async () => {
    withPlugins([VIEWER], {});
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    expect(wrapper.find("[data-test='open-with']").exists()).toBe(false);
    await wrapper.find("[data-test='actions']").trigger("click");
    const bubble = wrapper.findAllComponents(MessageBubble).find((one) => one.props("message").id === "m4");
    bubble?.vm.$emit("actions", "m4");
    await flushPromises();
    await wrapper.find("[data-test='open-with']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='open-with-com.flickertalk.pdfviewer']").exists()).toBe(true);
    expect(wrapper.find("[data-test='open-elsewhere']").text()).toContain("Another app");
    await wrapper.find("[data-test='open-elsewhere']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
  });

  it("folds and unfolds the message it was asked about", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);

    await wrapper.find("[data-test='fold']").trigger("click");
    expect(wrapper.findAllComponents(MessageBubble)[0].props("folded")).toBe(true);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);

    await pressed(wrapper);
    await wrapper.find("[data-test='fold']").trigger("click");
    expect(wrapper.findAllComponents(MessageBubble)[0].props("folded")).toBe(false);
  });

  it("hands a message to another app", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    const id = fixture.chats[0].messages[0].id;

    await wrapper.find("[data-test='share']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_share_message", { message: id }]);
  });

  it("erases a message only after asking", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    const id = fixture.chats[0].messages[0].id;

    await wrapper.find("[data-test='delete']").trigger("click");
    expect(calls.map(([command]) => command)).not.toContain("core_forget_message");

    await wrapper.find("[data-test='delete-sure']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_forget_message", { message: id }]);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
  });

  it("sends a message on to another contact", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    const id = fixture.chats[0].messages[0].id;

    await wrapper.find("[data-test='forward']").trigger("click");
    const other = fixture.chats[1].id;
    expect(wrapper.find(`[data-test='to-${other}']`).exists()).toBe(true);
    expect(wrapper.find(`[data-test='to-c1']`).exists()).toBe(false);

    await wrapper.find(`[data-test='to-${other}']`).trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_forward", { message: id, contact: other }]);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
  });

  describe("sending later", () => {
    const pad = (n: number) => String(n).padStart(2, "0");
    const local = (at: Date) => `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}T${pad(at.getHours())}:${pad(at.getMinutes())}`;
    const button = (wrapper: ReturnType<typeof mount>, test: string) =>
      wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === test)!;
    const opened = async (text: string) => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      expect(wrapper.find("[data-test='send-later']").exists()).toBe(false);
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", text);
      await flushPromises();
      await wrapper.find("[data-test='send-later']").trigger("click");
      return wrapper;
    };

    const scheduleSheet = (wrapper: ReturnType<typeof mount>) =>
      wrapper.findAllComponents(IonModalStub).find((one) => one.props("isOpen") && one.find("[data-test='schedule']").exists())!;
    const presented = async (text: string) => {
      const wrapper = await opened(text);
      scheduleSheet(wrapper).vm.$emit("didPresent");
      await flushPromises();
      return wrapper;
    };

    it("builds the date picker only once the sheet has presented", async () => {
      const wrapper = await opened("later");
      expect(wrapper.find("[data-test='schedule']").exists()).toBe(true);
      expect(wrapper.findComponent(IonDatetime).exists()).toBe(false);
      scheduleSheet(wrapper).vm.$emit("didPresent");
      await flushPromises();
      expect(wrapper.findComponent(IonDatetime).exists()).toBe(true);
      scheduleSheet(wrapper).vm.$emit("didDismiss");
      await flushPromises();
      expect(wrapper.findComponent(IonDatetime).exists()).toBe(false);
    });

    it("keeps the picker's room in the sheet before the picker is built", async () => {
      const wrapper = await opened("later");
      const room = wrapper.find("[data-test='schedule'] [data-test='schedule-room']");
      expect(room.exists()).toBe(true);
      expect(room.classes()).toContain("ft-schedule__room");
      expect(wrapper.findComponent(IonDatetime).exists()).toBe(false);
      scheduleSheet(wrapper).vm.$emit("didPresent");
      await flushPromises();
      expect(wrapper.find("[data-test='schedule-room'] [data-test='schedule-at']").exists()).toBe(true);
      const styles = source.slice(source.indexOf("<style"));
      const rule = [...styles.matchAll(/([^{}]+)\{([^{}]*)\}/g)].find(([, selector]) => selector.replace(/\/\*[\s\S]*?\*\//g, "").trim() === ".ft-schedule__room");
      expect(rule?.[2]).toMatch(/min-height:\s*var\(--ft-schedule-picker-height/);
    });

    it("lets Schedule be tapped only once the picker is there", async () => {
      const wrapper = await opened("later");
      expect(button(wrapper, "schedule-go").props("disabled")).toBe(true);
      scheduleSheet(wrapper).vm.$emit("didPresent");
      await flushPromises();
      expect(button(wrapper, "schedule-go").props("disabled")).toBe(false);
    });

    it("offers the clock only with words to send, and schedules them at the chosen time", async () => {
      const wrapper = await presented("good morning");
      const sheet = scheduleSheet(wrapper);
      expect(sheet.attributes("aria-label")).toBe("Send later");
      expect(sheet.props("breakpoints")).toContain(sheet.props("initialBreakpoint"));
      const picker = wrapper.findComponent(IonDatetime);
      expect(picker.attributes("data-test")).toBe("schedule-at");
      const at = new Date(Date.now() + 2 * 3_600_000);
      at.setSeconds(0, 0);
      picker.vm.$emit("update:modelValue", local(at));
      await flushPromises();
      await button(wrapper, "schedule-go").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_schedule", { contact: "c1", text: "good morning", sendAt: at.getTime() }]);
      expect(calls.filter(([command]) => command === "core_send")).toEqual([]);
      expect(wrapper.find("[data-test='schedule']").exists()).toBe(false);
    });

    it("picks the time in the app's language", async () => {
      await setLocale("es");
      try {
        const wrapper = await presented("buenos días");
        const picker = wrapper.findComponent(IonDatetime);
        expect(picker.props("locale")).toBe("es");
        expect(picker.find("[slot='time-label']").text()).toBe("Hora");
        expect(picker.props("firstDayOfWeek")).toBe(1);
      } finally {
        await setLocale("en");
      }
    });

    it("starts the week where the phone's region does", async () => {
      const phone = vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
      try {
        const wrapper = await presented("good morning");
        expect(wrapper.findComponent(IonDatetime).props("firstDayOfWeek")).toBe(0);
      } finally {
        phone.mockRestore();
      }
    });

    it("refuses a time that is too soon, and can be dropped", async () => {
      const wrapper = await presented("soon");
      wrapper.findComponent(IonDatetime).vm.$emit("update:modelValue", local(new Date(Date.now() - 60_000)));
      await flushPromises();
      expect(button(wrapper, "schedule-go").props("disabled")).toBe(true);
      await button(wrapper, "schedule-cancel").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='schedule']").exists()).toBe(false);
      expect(calls.filter(([command]) => command === "core_schedule")).toEqual([]);
    });
  });

  describe("searching the conversation", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    const typed = async (wrapper: ReturnType<typeof mount>, text: string) => {
      wrapper.findComponent(IonSearchbar).vm.$emit("update:modelValue", text);
      await wrapper.vm.$nextTick();
      vi.advanceTimersByTime(300);
      await flushPromises();
    };

    it("has no search button in the header", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='search']").exists()).toBe(false);
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(false);
    });

    it("opens when asked, lists what it finds and goes to a hit", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", search: true }, shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(true);

      await typed(wrapper, "TABLE");
      expect(calls).toContainEqual(["core_search", { contact: "c1", query: "TABLE" }]);
      const hits = wrapper.find("[data-test='search-hits']");
      expect(hits.text()).toContain("Perfect, I'll book the table");
      expect(hits.text()).toContain("Maria López");

      const target = document.createElement("div");
      target.dataset.message = "m3";
      target.scrollIntoView = vi.fn();
      document.body.append(target);
      await wrapper.find("[data-test='hit-m3']").trigger("click");
      expect(target.scrollIntoView).toHaveBeenCalled();
      target.remove();
      expect(wrapper.findAllComponents(MessageBubble).find((one) => one.props("message").id === "m3")?.classes()).toContain("is-lit");
    });

    it("opens when asked by a conversation already on screen", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      await wrapper.setProps({ search: true });
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(true);
    });

    it("opens when its page asks", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      (wrapper.vm as unknown as { openSearch: () => void }).openSearch();
      await wrapper.vm.$nextTick();
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(true);
    });

    it("says when nothing is found, and closes", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", search: true }, shallow: true });
      await flushPromises();
      await typed(wrapper, "zebra");
      expect(wrapper.find("[data-test='search-none']").exists()).toBe(true);
      await typed(wrapper, "");
      expect(wrapper.find("[data-test='search-none']").exists()).toBe(false);
      await wrapper.find("[data-test='close-search']").trigger("click");
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(false);
    });
  });

  describe("editing and taking back", () => {
    const pressOn = async (wrapper: ReturnType<typeof mount>, index: number) => {
      await wrapper.findAll("[data-test='bubble']")[index].trigger("pointerdown");
      await new Promise((wake) => setTimeout(wake, 550));
      await flushPromises();
    };

    it("puts my text in the composer and sends the new words as an edit", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressOn(wrapper, 1);
      expect(wrapper.find("[data-test='edit']").exists()).toBe(true);
      await wrapper.find("[data-test='edit']").trigger("click");
      const bar = wrapper.find("[data-test='editing']");
      expect(bar.text()).toContain("Yes! Leaving work at 5:30");
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "Yes! Leaving work at 6");
      await flushPromises();
      await wrapper.find("[aria-label='Send']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_edit", { message: "m2", text: "Yes! Leaving work at 6" }]);
      expect(calls.filter(([command]) => command === "core_send")).toEqual([]);
      expect(wrapper.find("[data-test='editing']").exists()).toBe(false);
    });

    it("can drop the edit, and offers none on their message or on my file", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressOn(wrapper, 1);
      await wrapper.find("[data-test='edit']").trigger("click");
      await wrapper.find("[data-test='cancel-edit']").trigger("click");
      expect(wrapper.find("[data-test='editing']").exists()).toBe(false);
      await pressOn(wrapper, 0);
      expect(wrapper.find("[data-test='edit']").exists()).toBe(false);
      await wrapper.find("[data-test='actions']").trigger("click");
      await pressOn(wrapper, 3);
      expect(wrapper.find("[data-test='edit']").exists()).toBe(false);
    });

    it("takes my message back for both sides after asking, and never theirs", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressOn(wrapper, 1);
      await wrapper.find("[data-test='delete']").trigger("click");
      expect(wrapper.find("[data-test='delete-everyone']").exists()).toBe(true);
      await wrapper.find("[data-test='delete-everyone']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_delete_everyone", { message: "m2" }]);
      expect(wrapper.find("[data-test='actions']").exists()).toBe(false);

      await pressOn(wrapper, 0);
      await wrapper.find("[data-test='delete']").trigger("click");
      expect(wrapper.find("[data-test='delete-sure']").exists()).toBe(true);
      expect(wrapper.find("[data-test='delete-everyone']").exists()).toBe(false);
    });

    it("offers only deleting it here for a text written for later", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      const mine = chat("c1")!.messages[1];
      mine.status = "pending";
      mine.scheduledFor = Date.now() + 3_600_000;
      await flushPromises();
      await pressOn(wrapper, 1);
      await wrapper.find("[data-test='delete']").trigger("click");
      expect(wrapper.find("[data-test='delete-everyone']").exists()).toBe(false);
      await wrapper.find("[data-test='delete-sure']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_forget_message", { message: mine.id }]);
    });
  });

  describe("pinned messages", () => {
    it("pins a message from the sheet, and unpins a pinned one", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressed(wrapper);
      const first = chat("c1")!.messages[0];
      expect(wrapper.find("[data-test='pin']").attributes("aria-label")).toBe("Pin");
      await wrapper.find("[data-test='pin']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_pin", { message: first.id, pinned: true }]);

      first.pinned = true;
      await flushPromises();
      await pressed(wrapper);
      expect(wrapper.find("[data-test='pin']").attributes("aria-label")).toBe("Unpin");
      await wrapper.find("[data-test='pin']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_pin", { message: first.id, pinned: false }]);
    });

    it("shows the latest pinned message in a strip and walks through them on a tap", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
      await flushPromises();
      expect(wrapper.find("[data-test='pinned-strip']").exists()).toBe(false);
      const [first, second] = chat("c1")!.messages;
      first.pinned = true;
      second.pinned = true;
      await flushPromises();
      const strip = wrapper.find("[data-test='pinned-strip']");
      expect(strip.text()).toContain(second.text);
      expect(strip.text()).toContain("1/2");

      const target = document.createElement("div");
      target.dataset.message = second.id;
      target.scrollIntoView = vi.fn();
      document.body.append(target);
      await strip.trigger("click");
      expect(target.scrollIntoView).toHaveBeenCalled();
      target.remove();
      expect(wrapper.find("[data-test='pinned-strip']").text()).toContain(first.text);
      expect(wrapper.find("[data-test='pinned-strip']").text()).toContain("2/2");
    });
  });

  describe("reacting to a message", () => {
    it("offers a row of emoji and puts the chosen one on the message", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressed(wrapper);
      const id = fixture.chats[0].messages[0].id;
      expect(wrapper.findAll(".ft-actions__react")).toHaveLength(6);
      await wrapper.find("[data-test='react-👍']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_react", { contact: "c1", message: id, emoji: "👍" }]);
      expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
    });

    it("takes the emoji back when it is already there", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      const first = chat("c1")!.messages[0];
      first.reactions = { mine: "👍" };
      await flushPromises();
      await pressed(wrapper);
      const lit = wrapper.find("[data-test='react-👍']");
      expect(lit.classes()).toContain("is-active");
      expect(lit.attributes("aria-label")).toBe("Remove reaction");
      await lit.trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_react", { contact: "c1", message: first.id }]);
    });
  });

  describe("Ionic controls of the chat features", () => {
    const clear = (wrapper: ReturnType<typeof mount>, test: string) => {
      const button = wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === test);
      expect(button, test).toBeDefined();
      expect(button!.props("fill"), test).toBe("clear");
      expect(button!.attributes("aria-label"), test).toBeTruthy();
      return button!;
    };
    const pressOn = async (wrapper: ReturnType<typeof mount>, index: number) => {
      await wrapper.findAll("[data-test='bubble']")[index].trigger("pointerdown");
      await new Promise((wake) => setTimeout(wake, 550));
      await flushPromises();
    };

    it("edits, pins and answers with Ionic's clear icon buttons, and drops them the same way", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressOn(wrapper, 1);
      clear(wrapper, "pin");
      clear(wrapper, "reply");
      await clear(wrapper, "edit").trigger("click");
      await clear(wrapper, "cancel-edit").trigger("click");
      expect(wrapper.find("[data-test='editing']").exists()).toBe(false);
      await pressOn(wrapper, 1);
      await clear(wrapper, "reply").trigger("click");
      await clear(wrapper, "cancel-reply").trigger("click");
      expect(wrapper.find("[data-test='replying']").exists()).toBe(false);
    });

    it("searches with Ionic's searchbar and closes with a clear icon button", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", search: true }, shallow: false, global: { stubs } });
      await flushPromises();
      const bar = wrapper.findComponent(IonSearchbar);
      expect(bar.attributes("data-test")).toBe("search-input");
      expect(bar.attributes("aria-label")).toBe("Search in this conversation");
      expect(wrapper.find("[data-test='search-panel'] input").exists()).toBe(false);
      await clear(wrapper, "close-search").trigger("click");
      expect(wrapper.find("[data-test='search-panel']").exists()).toBe(false);
    });

    it("styles them with Ionic's colours and logical sides", () => {
      const styles = source.slice(source.indexOf("<style"));
      const rules = [...styles.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
        .map(([, selector, body]) => ({ selector: selector.trim(), body }))
        .filter(({ selector }) => /\.ft-(schedule|search|pinned|replying|actions__emoji|actions__react)|is-typing|is-lit/.test(selector));
      expect(rules.length).toBeGreaterThan(5);
      for (const { selector, body } of rules) {
        expect(body, selector).not.toMatch(/var\(--ft-(accent|muted|text|surface|border|bg)/);
        expect(body, selector).not.toMatch(/color-mix|rgba\(\s*\d/);
        expect(body, selector).not.toMatch(/(padding|margin)(-left|-right)?:\s*\S+\s+\S+\s+\S+\s+\S+;|(padding|margin)-(left|right)|\b(left|right):/);
      }
    });
  });

  describe("answering a message", () => {
    const open = async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressed(wrapper);
      const first = fixture.chats[0].messages[0];
      return { wrapper, first };
    };

    it("offers to answer, shows the quote in the composer and sends the text with it", async () => {
      const { wrapper, first } = await open();
      await wrapper.find("[data-test='reply']").trigger("click");
      expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
      const bar = wrapper.find("[data-test='replying']");
      expect(bar.exists()).toBe(true);
      expect(bar.text()).toContain(first.text ?? "");
      expect(bar.text()).toContain(first.mine ? "You" : "Maria López");

      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "yes!");
      await flushPromises();
      await wrapper.find("[aria-label='Send']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_send", { contact: "c1", text: "yes!", replyTo: first.id }]);
      expect(wrapper.find("[data-test='replying']").exists()).toBe(false);
    });

    it("can drop the answer, and then sends a plain text", async () => {
      const { wrapper } = await open();
      await wrapper.find("[data-test='reply']").trigger("click");
      await wrapper.find("[data-test='cancel-reply']").trigger("click");
      expect(wrapper.find("[data-test='replying']").exists()).toBe(false);
      wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hi");
      await flushPromises();
      await wrapper.find("[aria-label='Send']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_send", { contact: "c1", text: "hi" }]);
    });

    it("gives each bubble the contact's name for its quote", async () => {
      const { wrapper } = await open();
      const bubbles = wrapper.findAllComponents(MessageBubble);
      expect(bubbles.length).toBeGreaterThan(0);
      for (const bubble of bubbles) expect(bubble.props("contactName")).toBe("Maria López");
    });

    it("goes to the quoted message when its quote is tapped", async () => {
      const { wrapper } = await open();
      const target = document.createElement("div");
      target.dataset.message = "m-far";
      target.scrollIntoView = vi.fn();
      document.body.append(target);
      wrapper.findAllComponents(MessageBubble)[0].vm.$emit("jump", "m-far");
      expect(target.scrollIntoView).toHaveBeenCalled();
      target.remove();
    });
  });

  it("offers to forward to contacts whose names read in their own direction", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    await wrapper.find("[data-test='forward']").trigger("click");
    expect(wrapper.find(`[data-test='to-${fixture.chats[1].id}']`).attributes("dir")).toBe("auto");
  });

  it("keeps the last message in sight when the keyboard opens", async () => {
    const viewport = new FakeViewport();
    Object.defineProperty(window, "visualViewport", { value: viewport, configurable: true });
    Object.defineProperty(window, "innerHeight", { value: 900, configurable: true });
    const stop = startViewportFit(window, document.documentElement);
    const scroller = { scrollHeight: 2000, clientHeight: 800, scrollTop: 1200 };
    const wrapper = mount(ChatThread, {
      props: { chatId: "c1" },
      shallow: true,
      global: { stubs: { IonContent: contentWith(scroller) } },
    });
    await flushPromises();
    viewport.move(560);
    expect(scroller.scrollTop).toBe(2000);

    wrapper.unmount();
    scroller.scrollTop = 1200;
    viewport.move(900);
    viewport.move(560);
    expect(scroller.scrollTop).toBe(1200);
    stop();
    document.documentElement.removeAttribute("style");
  });

  describe("games", () => {
    const CODE = {
      id: "com.flickertalk.code",
      name: "Code block",
      version: "1.0.0",
      asks: { network: [], messages: true, send: "nothing" },
      granted: { network: [], messages: true, send: "nothing" },
      installedAt: 1,
    };
    const CHESS = {
      id: "com.flickertalk.game.chess",
      name: "Chess",
      version: "1.0.0",
      kind: "game",
      asks: { network: [], messages: false, send: "propose", live: true },
      granted: { network: [], messages: false, send: "propose", live: true },
      installedAt: 2,
    };
    const UNGRANTED = { ...CHESS, granted: { network: [], messages: false, send: "nothing", live: false } };
    const GO = { id: "com.flickertalk.game.go", name: "Go", version: "1.0.0", summary: "Play go.", size: 1_200_000, installed: false, carried: false, kind: "game" };
    const IPHONE = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";

    function bridge({
      installed = [] as unknown[],
      catalogue = [] as unknown[],
      messages = [] as unknown[],
      addFails = false,
      opensChat = null as string | null,
    } = {}) {
      let list = installed as Array<Record<string, unknown>>;
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_plugins") return list;
        if (command === "core_plugin_open_chat") return opensChat ? { contact: opensChat, message: "m1" } : null;
        if (command === "core_catalogue") return catalogue;
        if (command === "core_messages") return messages;
        if (command === "core_plugin_add") {
          if (addFails) throw new Error("download failed");
          const entry = (catalogue as Array<{ id: string; name: string }>).find((one) => one.id === args?.plugin)!;
          list = [...list, { ...UNGRANTED, id: entry.id, name: entry.name }];
        }
        if (command === "core_plugin_grant") list = list.map((one) => (one.id === args?.plugin ? { ...one, granted: args?.granted } : one));
        return undefined;
      });
      void refreshPlugins();
    }

    const thread = async (props: Record<string, unknown> = {}) => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", ...props }, shallow: false, global: { stubs } });
      await flushPromises();
      return wrapper;
    };
    const openGames = async (wrapper: Awaited<ReturnType<typeof thread>>) => {
      await wrapper.find("[data-test='apps']").trigger("click");
      wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
      await flushPromises();
    };
    const endButtons = (wrapper: Awaited<ReturnType<typeof thread>>) => wrapper.findAll(".ft-thread__bar ion-buttons[slot='end'] ion-button");
    const selected = (wrapper: Awaited<ReturnType<typeof thread>>) => wrapper.findComponent(IonSegment).props("value");

    beforeEach(() => {
      offered.value = [];
      push.mockClear();
    });
    afterEach(() => vi.restoreAllMocks());

    it("opens a sheet with a segment for the tools and one for the games", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      expect(wrapper.find("[data-test='games']").exists()).toBe(false);
      expect(endButtons(wrapper)).toHaveLength(3);
      expect(wrapper.find("[data-test='search']").exists()).toBe(false);
      expect(wrapper.find("[data-test='apps-sheet']").exists()).toBe(false);
      await wrapper.find("[data-test='apps']").trigger("click");
      const sheet = wrapper.findComponent(IonModalStub);
      expect(sheet.attributes("aria-label")).toBe("Apps");
      expect(sheet.props("isOpen")).toBe(true);
      expect(sheet.props("breakpoints")).toContain(sheet.props("initialBreakpoint"));
      const segments = wrapper.findAllComponents(IonSegmentButton);
      expect(segments.map((one) => [one.props("value"), one.find("ion-label").element.innerHTML])).toEqual([
        ["tools", "Tools"],
        ["games", "Games"],
      ]);
      expect(selected(wrapper)).toBe("tools");
      expect(wrapper.find(`[data-test='app-${CODE.id}']`).exists()).toBe(true);
      expect(wrapper.find(`[data-test='app-${CHESS.id}']`).exists()).toBe(false);
      const tiles = () => wrapper.findAllComponents(AppTile).map((one) => [one.attributes("data-test"), one.props("name"), Boolean(one.props("off"))]);
      expect(tiles()).toEqual([[`app-${CODE.id}`, "Code block", false]]);
      expect(wrapper.find("[data-test='apps-sheet-tools']").classes()).toContain("ft-app-grid");
      wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
      await flushPromises();
      expect(selected(wrapper)).toBe("games");
      expect(wrapper.find("[data-test='games-sheet']").html()).toContain("Chess");
      expect(wrapper.find(`[data-test='app-${CODE.id}']`).exists()).toBe(false);
      expect(tiles()).toEqual([[`game-${CHESS.id}`, "Chess", false]]);
      expect(wrapper.find("[data-test='games-sheet']").classes()).toContain("ft-app-grid");
    });

    it("opens on the games when there is no tool, and is there with nothing installed", async () => {
      bridge({ installed: [CHESS] });
      const games = await thread();
      await games.find("[data-test='apps']").trigger("click");
      expect(selected(games)).toBe("games");
      expect(games.find("[data-test='games-sheet']").html()).toContain("Chess");

      bridge({ installed: [] });
      const empty = await thread();
      await empty.find("[data-test='apps']").trigger("click");
      expect(selected(empty)).toBe("games");
      expect(empty.find("[data-test='games-sheet']").html()).toContain("No games yet");
    });

    it("remembers nothing between two openings", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      await openGames(wrapper);
      wrapper.findComponent(IonModalStub).vm.$emit("didDismiss");
      await flushPromises();
      expect(wrapper.find("[data-test='apps-sheet']").exists()).toBe(false);
      await wrapper.find("[data-test='apps']").trigger("click");
      expect(selected(wrapper)).toBe("tools");
    });

    it("closes when a plugin is chosen, and opens it", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      await wrapper.find("[data-test='apps']").trigger("click");
      await wrapper.find(`[data-test='app-${CODE.id}']`).trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='apps-sheet']").exists()).toBe(false);
      expect(wrapper.findComponent({ name: "PluginSheet" }).props("plugin")).toMatchObject({ id: CODE.id });
    });

    it("has the games segment on an iPhone too, and the button without a tool", async () => {
      vi.spyOn(navigator, "userAgent", "get").mockReturnValue(IPHONE);
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      await wrapper.find("[data-test='apps']").trigger("click");
      expect(wrapper.findComponent(IonSegment).exists()).toBe(true);
      wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
      await flushPromises();
      expect(wrapper.find("[data-test='games-sheet']").html()).toContain("Chess");

      bridge({ installed: [] });
      expect((await thread()).find("[data-test='apps']").exists()).toBe(true);
    });

    it("shows the installed games, and the way to more", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      await openGames(wrapper);
      const sheet = wrapper.find("[data-test='games-sheet']");
      expect(sheet.html()).toContain("Chess");
      expect(sheet.html()).not.toContain("Code block");
      await wrapper.find("[data-test='more-games-link']").trigger("click");
      expect(push).toHaveBeenCalledWith("/tabs/apps?show=games");
    });

    it("says there is no game yet, and still leads to more", async () => {
      bridge({ installed: [CODE] });
      const wrapper = await thread();
      await openGames(wrapper);
      expect(wrapper.find("[data-test='games-sheet']").html()).toContain("No games yet");
      expect(wrapper.find("[data-test='more-games-link']").exists()).toBe(true);
    });

    it("plays a game in this conversation, with the live channel", async () => {
      bridge({ installed: [CHESS] });
      const wrapper = await thread();
      await openGames(wrapper);
      await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
      await flushPromises();
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.props("plugin")).toMatchObject({ id: CHESS.id, name: "Chess" });
      expect(sheet.props("live")).toBe(true);
      expect(sheet.props("sending")).toBe("propose");
      expect(sheet.props("contact")).toBe("c1");
    });

    it("plays a game when the tools are locked", async () => {
      bridge({ installed: [CODE, CHESS] });
      const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
      const answer = internals.invoke;
      internals.invoke = (command, args) => (command === "core_plan" ? Promise.resolve({ state: "limited", until: 0 }) : answer(command, args));
      const wrapper = await thread();
      await openGames(wrapper);
      await flushPromises();
      expect(wrapper.findAllComponents(AppTile).find((one) => one.attributes("data-test") === `game-${CHESS.id}`)?.props("badge")).toBeUndefined();
      await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "PluginSheet" }).props("plugin")).toMatchObject({ id: CHESS.id });
      premiumLocked.value = false;
    });

    it("draws a game's own image on its tile and on its question", async () => {
      const image = '<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>';
      bridge({ installed: [{ ...UNGRANTED, image }] });
      const wrapper = await thread();
      await openGames(wrapper);
      const chess = wrapper.findAllComponents(AppTile).find((one) => one.attributes("data-test") === `game-${CHESS.id}`)!;
      expect(chess.props("image")).toBe(pluginImage({ image }));
      await chess.trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "GamePermissions" }).props("image")).toBe(pluginImage({ image }));
    });

    it("asks with the icon the game names", async () => {
      bridge({ installed: [{ ...UNGRANTED, icon: "shield-outline" }] });
      const wrapper = await thread();
      await openGames(wrapper);
      await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "GamePermissions" }).props("icon")).toBe(shieldOutline);
    });

    it("asks first what a game needs, and opens nothing if refused", async () => {
      bridge({ installed: [UNGRANTED] });
      const wrapper = await thread();
      await openGames(wrapper);
      await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='game-permissions']").exists()).toBe(true);
      await wrapper.find("[data-test='game-cancel']").trigger("click");
      await flushPromises();
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_grant");

      await openGames(wrapper);
      await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
      await wrapper.find("[data-test='game-allow']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual([
        "core_plugin_grant",
        { plugin: CHESS.id, granted: { network: [], messages: false, send: "propose", live: true } },
      ]);
      expect(wrapper.findComponent({ name: "PluginSheet" }).props("live")).toBe(true);
    });

    it("opens the game the address asks for", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread({ play: CHESS.id });
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.props("plugin")).toMatchObject({ id: CHESS.id });
      expect(sheet.props("live")).toBe(true);
    });

    it("opens the game the address asked for even when the address has forgotten it", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = mount(ChatThread, { props: { chatId: "c1", play: CHESS.id }, shallow: false, global: { stubs } });
      await wrapper.setProps({ play: undefined });
      await flushPromises();
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.exists()).toBe(true);
      expect(sheet.props("plugin")).toMatchObject({ id: CHESS.id });
    });

    it("opens the game again when the address asks for it again", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread({ play: CHESS.id });
      await wrapper.setProps({ play: undefined });
      await wrapper.find("[data-test='close-game']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='game-room']").exists()).toBe(false);
      await wrapper.setProps({ play: CHESS.id });
      await flushPromises();
      expect(wrapper.find("[data-test='game-room']").exists()).toBe(true);
    });

    it("asks first when the game the address asks for lacks what it needs, and never opens a tool", async () => {
      bridge({ installed: [CODE, UNGRANTED] });
      const asked = await thread({ play: CHESS.id });
      expect(asked.find("[data-test='game-permissions']").exists()).toBe(true);
      expect(asked.findComponent({ name: "PluginSheet" }).exists()).toBe(false);

      const tool = await thread({ play: CODE.id });
      expect(tool.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    });

    it("leaves an invitation in the composer, without sending it, when a game's tile is held", async () => {
      bridge({ installed: [CHESS] });
      const wrapper = await thread();
      await openGames(wrapper);
      const chess = wrapper.findAllComponents(AppTile).find((one) => one.attributes("data-test") === `game-${CHESS.id}`)!;
      expect(chess.attributes("aria-label")).toBe("Chess. Touch and hold to invite");
      chess.vm.$emit("hold");
      await flushPromises();
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("🎮 Chess · Shall we play? https://flickertalk.com/games/chess");
      expect(wrapper.find("[data-test='games-sheet']").exists()).toBe(false);
      expect(calls.map(([command]) => command)).not.toContain("core_send");
    });

    const INVITE = { id: "g1", outgoing: false, text: "🎮 Shall we play Go? https://flickertalk.com/games/go", sentAt: 0, state: "delivered" };

    it("names a game in the phone's language wherever it is shown", async () => {
      await setLocale("es");
      try {
        bridge({ installed: [UNGRANTED] });
        offered.value = [{ ...GO, id: CHESS.id, name: "Chess", installed: true, locales: { es: { name: "Ajedrez" } } } as never];
        const wrapper = await thread();
        await openGames(wrapper);
        expect(wrapper.find(`[data-test='game-${CHESS.id}'] .ft-app-tile__name`).text()).toBe("Ajedrez");
        await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='game-permissions']").text()).toContain("Ajedrez");
        await wrapper.find("[data-test='game-allow']").trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='game-room']").attributes("aria-label")).toBe("Ajedrez");
        expect(wrapper.find("[data-test='game-bar']").text()).toContain("Ajedrez");
        expect(wrapper.findComponent({ name: "PluginSheet" }).props("plugin")).toMatchObject({ name: "Ajedrez" });
        expect(wrapper.findComponent({ name: "PluginSheet" }).props("fill")).toBe(false);
        await wrapper.find("[data-test='game-invite']").trigger("click");
        await flushPromises();
        const invitation = wrapper.findComponent(IonTextarea).props("modelValue") as string;
        expect(invitation).toContain("Ajedrez");
        expect(invitation).not.toContain("Chess");
        expect(invitation.endsWith("https://flickertalk.com/games/chess")).toBe(true);
      } finally {
        await setLocale("en");
      }
    });

    it("installs, grants and opens the game an invitation is for", async () => {
      bridge({ installed: [CODE], catalogue: [GO], messages: [INVITE] });
      const wrapper = await thread();
      expect(calls.filter(([command]) => command === "core_catalogue")).toHaveLength(1);
      const bubble = wrapper.findAllComponents(MessageBubble).find((one) => one.props("message").id === "g1")!;
      expect(bubble.props("games")).toBe(true);
      bubble.vm.$emit("play", GO.id);
      await flushPromises();
      const ask = wrapper.find("[data-test='game-permissions']");
      expect(ask.text()).toContain("Go");
      expect(ask.text()).toContain("1.2 MB");
      await wrapper.find("[data-test='game-allow']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_plugin_add", { plugin: GO.id }]);
      expect(calls).toContainEqual(["core_plugin_grant", { plugin: GO.id, granted: { network: [], messages: false, send: "propose", live: true } }]);
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.props("plugin")).toMatchObject({ id: GO.id });
      expect(sheet.props("live")).toBe(true);
    });

    it("says so when the game of an invitation cannot be installed", async () => {
      bridge({ installed: [], catalogue: [GO], messages: [INVITE], addFails: true });
      const wrapper = await thread();
      wrapper.findAllComponents(MessageBubble)[0].vm.$emit("play", GO.id);
      await flushPromises();
      await wrapper.find("[data-test='game-allow']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[role='alert']").text()).toBe("The game could not be installed. Check your connection and try again.");
      expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    });

    it("does not read the catalogue for a conversation without invitations", async () => {
      bridge({ installed: [CHESS], messages: [{ ...INVITE, text: "https://flickertalk.com/games/chess" }] });
      await thread();
      bridge({ installed: [CHESS], messages: [{ ...INVITE, text: "see you at six" }] });
      await thread();
      expect(calls.map(([command]) => command)).not.toContain("core_catalogue");
    });

    describe("the game room", () => {
      const playChess = async (installed: unknown[] = [CODE, CHESS]) => {
        bridge({ installed });
        const wrapper = await thread({ play: CHESS.id });
        return wrapper;
      };
      const room = (wrapper: Awaited<ReturnType<typeof thread>>) => wrapper.find("[data-test='game-room']");
      const hidden = (element: { attributes: (name: string) => string | undefined }) => /display:\s*none/.test(element.attributes("style") ?? "");
      const arrives = (text: string) => {
        chat("c1")!.messages.push({ id: `in-${text}`, mine: false, text, time: "10:00", sentAt: Date.now() });
      };

      it("opens a game between the header and the composer, the thread hidden but kept", async () => {
        const wrapper = await playChess();
        expect(room(wrapper).exists()).toBe(true);
        expect(room(wrapper).findComponent({ name: "PluginSheet" }).props("plugin")).toMatchObject({ id: CHESS.id });
        expect(wrapper.find(".ft-app").exists()).toBe(false);
        expect(wrapper.find("[data-test='peer']").exists()).toBe(true);
        expect(wrapper.findComponent(IonTextarea).exists()).toBe(true);
        const messages = wrapper.find(".ft-thread__content");
        expect(messages.exists()).toBe(true);
        expect(hidden(messages)).toBe(true);
      });

      it("still opens a tool in its own full-screen window", async () => {
        bridge({ installed: [CODE, CHESS] });
        const wrapper = await thread();
        await wrapper.find("[data-test='apps']").trigger("click");
        await wrapper.find(`[data-test='app-${CODE.id}']`).trigger("click");
        await flushPromises();
        expect(wrapper.find(".ft-app").exists()).toBe(true);
        expect(room(wrapper).exists()).toBe(false);
      });

      it("has a bar with a way out, the game's name and an invitation that leaves the game open", async () => {
        const wrapper = await playChess();
        const bar = wrapper.find("[data-test='game-bar']");
        expect(bar.html()).toContain("Chess");
        await wrapper.find("[data-test='game-invite']").trigger("click");
        await flushPromises();
        expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("🎮 Chess · Shall we play? https://flickertalk.com/games/chess");
        expect(room(wrapper).exists()).toBe(true);

        await wrapper.find("[data-test='close-game']").trigger("click");
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
      });

      it("keeps the game open when it proposes a text or a file", async () => {
        const wrapper = await playChess();
        const sheet = wrapper.findComponent({ name: "PluginSheet" });
        sheet.vm.$emit("text", "♟️ Chess: I won");
        await flushPromises();
        expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("♟️ Chess: I won");
        expect(room(wrapper).exists()).toBe(true);

        sheet.vm.$emit("attach", { path: "/p/game.pgn", name: "game.pgn", mime: "application/x-chess-pgn", size: 9 });
        sheet.vm.$emit("done");
        await flushPromises();
        expect(wrapper.find("[data-test='staged']").exists()).toBe(true);
        expect(room(wrapper).exists()).toBe(true);

        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("done");
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
      });

      it("hides the attach button and the video call while playing, and brings them back after", async () => {
        const wrapper = await playChess();
        expect(wrapper.find(`[aria-label='Attach']`).exists()).toBe(false);
        expect(wrapper.find(`[aria-label='Video call']`).exists()).toBe(false);
        expect(wrapper.find(`[aria-label='Voice call']`).exists()).toBe(true);
        expect(wrapper.find("[data-test='open-emoji']").exists()).toBe(true);
        await wrapper.find("[data-test='close-game']").trigger("click");
        await flushPromises();
        expect(wrapper.find(`[aria-label='Attach']`).exists()).toBe(true);
        expect(wrapper.find(`[aria-label='Video call']`).exists()).toBe(true);
      });

      it("shows what the other one writes, and the thread on a tap, with the game still running", async () => {
        const wrapper = await playChess();
        const strip = () => wrapper.find("[data-test='game-strip']");
        expect(strip().exists()).toBe(true);
        expect(strip().html()).not.toContain("nice move");
        expect(strip().find("ion-label").element.innerHTML).toBe("Show the chat");
        arrives("nice move!");
        await flushPromises();
        expect(strip().html()).toContain("nice move!");

        await wrapper.find("[data-test='game-peek']").trigger("click");
        await flushPromises();
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
        expect(hidden(wrapper.find("[data-test='game-area']"))).toBe(true);
        expect(room(wrapper).findComponent({ name: "PluginSheet" }).exists()).toBe(true);

        await wrapper.find("[data-test='game-peek']").trigger("click");
        await flushPromises();
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(true);
        expect(hidden(wrapper.find("[data-test='game-area']"))).toBe(false);
        expect(strip().html()).not.toContain("nice move!");
      });

      it("shows the other one's line in its own direction", async () => {
        const wrapper = await playChess();
        arrives("¿otra?");
        await flushPromises();
        expect(wrapper.find("[data-test='game-strip'] ion-label").attributes("dir")).toBe("auto");
      });

      it("marks read what arrives while playing only when the thread is shown", async () => {
        const wrapper = await playChess();
        calls.length = 0;
        arrives("one");
        arrives("two");
        await flushPromises();
        expect(calls.map(([command]) => command)).not.toContain("core_mark_read");
        await wrapper.find("[data-test='game-peek']").trigger("click");
        await flushPromises();
        expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
      });

      it("shows the newest message when the game closes", async () => {
        bridge({ installed: [CODE, CHESS] });
        const scrollToBottom = vi.fn(async () => {});
        const Content = defineComponent({
          mounted() {
            (this.$el as { scrollToBottom?: unknown }).scrollToBottom = scrollToBottom;
          },
          render() {
            return h("div", this.$slots.default?.());
          },
        });
        const wrapper = mount(ChatThread, { props: { chatId: "c1", play: CHESS.id }, shallow: false, global: { stubs: { ...stubs, IonContent: Content } } });
        await flushPromises();
        expect(room(wrapper).exists()).toBe(true);
        arrives("your move");
        await flushPromises();
        scrollToBottom.mockClear();
        await wrapper.find("[data-test='close-game']").trigger("click");
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
        expect(scrollToBottom).toHaveBeenCalled();
      });

      it("closes the game with the back button and stays in the chat", async () => {
        const wrapper = await playChess();
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
      });
    });

    describe("asking for a permission on the spot", () => {
      const LOCATION = {
        ...CODE,
        id: "com.flickertalk.location",
        name: "Location",
        asks: { network: [], messages: false, send: "propose", location: true },
        granted: { network: [], messages: false, send: "nothing", location: false },
      };
      const openTool = async () => {
        bridge({ installed: [LOCATION, CHESS] });
        const wrapper = await thread();
        await wrapper.find("[data-test='apps']").trigger("click");
        await wrapper.find(`[data-test='app-${LOCATION.id}']`).trigger("click");
        await flushPromises();
        return wrapper;
      };
      const need = (key = "location") => ({ key, label: key === "location" ? "Your location, only when you ask" : "Write in the chat", icon: shieldOutline, on: false, answer: vi.fn() });
      const asks = (wrapper: Awaited<ReturnType<typeof thread>>) => wrapper.find("[data-test='permission-ask']");

      it("shows the tool and the one permission it needs, and hands a yes back", async () => {
        const wrapper = await openTool();
        expect(asks(wrapper).exists()).toBe(false);
        const asked = need();
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("needs", asked);
        await flushPromises();
        expect(asks(wrapper).html()).toContain("Location needs a permission");
        expect(asks(wrapper).html()).toContain("Your location, only when you ask");
        await wrapper.find("[data-test='permission-allow']").trigger("click");
        await flushPromises();
        expect(asked.answer).toHaveBeenCalledWith(true);
        expect(asks(wrapper).exists()).toBe(false);
        expect(wrapper.find(".ft-app").exists()).toBe(true);
      });

      it("hands a no back on Cancel", async () => {
        const wrapper = await openTool();
        const asked = need("send");
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("needs", asked);
        await flushPromises();
        expect(asks(wrapper).html()).toContain("Write in the chat");
        await wrapper.find("[data-test='permission-cancel']").trigger("click");
        await flushPromises();
        expect(asked.answer).toHaveBeenCalledWith(false);
        expect(asked.answer).toHaveBeenCalledTimes(1);
        expect(asks(wrapper).exists()).toBe(false);
      });

      it("takes Android's back button as a no, and leaves the tool open", async () => {
        const wrapper = await openTool();
        const asked = need();
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("needs", asked);
        await flushPromises();
        back.handler?.();
        await flushPromises();
        expect(asked.answer).toHaveBeenCalledWith(false);
        expect(asks(wrapper).exists()).toBe(false);
        expect(wrapper.find(".ft-app").exists()).toBe(true);
      });

      it("takes the question away when the tool closes", async () => {
        const wrapper = await openTool();
        const asked = need();
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("needs", asked);
        await flushPromises();
        await wrapper.find("[data-test='close-app']").trigger("click");
        await flushPromises();
        expect(asked.answer).toHaveBeenCalledWith(false);
        expect(asks(wrapper).exists()).toBe(false);
      });

      it("asks in the game room too", async () => {
        bridge({ installed: [CODE, CHESS] });
        const wrapper = await thread({ play: CHESS.id });
        const asked = { ...need("live"), label: "Talk to the same game on the other person's phone" };
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("needs", asked);
        await flushPromises();
        expect(asks(wrapper).html()).toContain("Chess needs a permission");
        await wrapper.find("[data-test='permission-allow']").trigger("click");
        expect(asked.answer).toHaveBeenCalledWith(true);
      });
    });

    describe("closing a plugin", () => {
      type Thread = Awaited<ReturnType<typeof thread>>;
      const sheets = (wrapper: Thread) => wrapper.findAllComponents({ name: "PluginSheet" });
      const hidden = (element: { attributes: (name: string) => string | undefined }) => /display:\s*none/.test(element.attributes("style") ?? "");

      async function up(wrapper: Thread) {
        const frame = wrapper.find("iframe").element as HTMLIFrameElement;
        const post = vi.fn();
        Object.defineProperty(frame, "contentWindow", { value: { postMessage: post }, configurable: true });
        const says = async (data: unknown) => {
          window.dispatchEvent(new MessageEvent("message", { data, source: frame.contentWindow }));
          await flushPromises();
        };
        await says({ type: "ft.ready" });
        const closings = () => post.mock.calls.filter(([message]) => message.type === "ft.closing").length;
        return { says, closings };
      }

      async function openTool(wrapper: Thread) {
        await wrapper.find("[data-test='apps']").trigger("click");
        await wrapper.find(`[data-test='app-${CODE.id}']`).trigger("click");
        await flushPromises();
      }

      async function withTool(options: Parameters<typeof bridge>[0] = {}) {
        bridge({ installed: [CODE, CHESS], ...options });
        const wrapper = await thread();
        await openTool(wrapper);
        return { wrapper, ...(await up(wrapper)) };
      }

      async function withGame() {
        bridge({ installed: [CODE, CHESS] });
        const wrapper = await thread({ play: CHESS.id });
        return { wrapper, ...(await up(wrapper)) };
      }

      it("hides a tool's window at once with ✕, and lets the plugin go once it has said goodbye", async () => {
        const { wrapper, says, closings } = await withTool();
        await wrapper.find("[data-test='close-app']").trigger("click");
        await flushPromises();
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find(".ft-app"))).toBe(true);
        expect(sheets(wrapper)).toHaveLength(1);

        await says({ type: "ft.closed" });
        expect(wrapper.find(".ft-app").exists()).toBe(false);
        expect(sheets(wrapper)).toHaveLength(0);
      });

      it("goes through the plugin with the back button, and stays in the chat", async () => {
        const { wrapper, says, closings } = await withTool();
        const sheet = sheets(wrapper)[0];
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find(".ft-app"))).toBe(true);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
        expect(sheet.emitted("closed")).toHaveLength(1);
        expect(back.handler).toBeNull();
        expect(push).not.toHaveBeenCalled();
      });

      it("keeps Back taken while the plugin says goodbye, however often it is pressed", async () => {
        const { wrapper, says, closings } = await withTool();
        const sheet = sheets(wrapper)[0];
        back.handler?.();
        await flushPromises();
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        back.handler?.();
        await flushPromises();
        expect(back.handler).not.toBeNull();
        expect(closings()).toBe(1);
        expect(sheets(wrapper)).toHaveLength(1);
        expect(push).not.toHaveBeenCalled();

        await says({ type: "ft.closed" });
        expect(sheet.emitted("closed")).toHaveLength(1);
        expect(sheets(wrapper)).toHaveLength(0);
        expect(back.handler).toBeNull();
        expect(push).not.toHaveBeenCalled();
      });

      it("holds Back during a goodbye only while the conversation is on screen", async () => {
        const { wrapper, says, closings } = await withTool();
        await wrapper.find("[data-test='close-app']").trigger("click");
        await flushPromises();
        expect(back.handler).not.toBeNull();

        await wrapper.setProps({ active: false });
        await flushPromises();
        expect(back.handler).toBeNull();

        await wrapper.setProps({ active: true });
        await flushPromises();
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        expect(back.handler).not.toBeNull();
        expect(closings()).toBe(1);
        expect(push).not.toHaveBeenCalled();

        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
        expect(back.handler).toBeNull();
      });

      it("keeps a plugin open, without a word to it, while its page is under another", async () => {
        const { wrapper, closings } = await withTool();
        await wrapper.setProps({ active: false });
        await flushPromises();
        expect(closings()).toBe(0);
        expect(sheets(wrapper)).toHaveLength(1);
        expect(hidden(wrapper.find(".ft-app"))).toBe(false);
        expect(back.handler).toBeNull();

        await wrapper.setProps({ active: true });
        await flushPromises();
        expect(back.handler).not.toBeNull();
        expect(closings()).toBe(0);
      });

      it("keeps a plugin whose page is under another when that page's chatId follows the address", async () => {
        const { wrapper, says, closings } = await withGame();
        await wrapper.setProps({ chatId: "c2" });
        await flushPromises();
        await wrapper.setProps({ active: false });
        await flushPromises();
        await wrapper.setProps({ chatId: "c1" });
        await wrapper.setProps({ active: true });
        await flushPromises();
        expect(closings()).toBe(0);
        expect(hidden(wrapper.find("[data-test='game-room']"))).toBe(false);
        calls.length = 0;
        await says({ type: "ft.liveSend", id: "q1", data: "aGk=" });
        expect(calls).toContainEqual(["core_plugin_live_send", { plugin: CHESS.id, contact: "c1", data: "aGk=" }]);
      });

      it("lets the page close the game as it goes, through the plugin", async () => {
        const { wrapper, says, closings } = await withGame();
        (wrapper.vm as unknown as { leave: () => void }).leave();
        await flushPromises();
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find("[data-test='game-room']"))).toBe(true);
        expect(sheets(wrapper)).toHaveLength(1);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
      });

      it("does not close twice when the page goes while Back's close is under way", async () => {
        const { wrapper, closings } = await withGame();
        back.handler?.();
        await flushPromises();
        (wrapper.vm as unknown as { leave: () => void }).leave();
        await flushPromises();
        expect(closings()).toBe(1);
      });

      it("has nothing to close when the page goes with nothing open", async () => {
        bridge({ installed: [CODE, CHESS] });
        const wrapper = await thread();
        expect(() => (wrapper.vm as unknown as { leave: () => void }).leave()).not.toThrow();
        expect(sheets(wrapper)).toHaveLength(0);
      });

      it("says goodbye once when it is closed twice", async () => {
        const { wrapper, says, closings } = await withTool();
        const sheet = sheets(wrapper)[0];
        await wrapper.find("[data-test='close-app']").trigger("click");
        await flushPromises();
        back.handler?.();
        await flushPromises();
        back.handler?.();
        await flushPromises();
        expect(back.handler).not.toBeNull();
        expect(closings()).toBe(1);
        expect(sheets(wrapper)).toHaveLength(1);
        expect(push).not.toHaveBeenCalled();
        await says({ type: "ft.closed" });
        expect(sheet.emitted("closed")).toHaveLength(1);
        expect(sheets(wrapper)).toHaveLength(0);
        expect(back.handler).toBeNull();
      });

      it("gives Back to a plugin opened while another said goodbye", async () => {
        const { wrapper, says } = await withGame();
        back.handler?.();
        await flushPromises();
        await openTool(wrapper);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)[0].props("plugin")).toMatchObject({ id: CODE.id });
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        expect(wrapper.find(".ft-app").exists()).toBe(false);
        expect(push).not.toHaveBeenCalled();
      });

      it("goes through the plugin when it asks to be closed (done)", async () => {
        const { wrapper, says, closings } = await withTool();
        await says({ type: "ft.close" });
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find(".ft-app"))).toBe(true);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
      });

      it("goes through the plugin when a tool proposes a text or a file", async () => {
        const text = await withTool();
        sheets(text.wrapper)[0].vm.$emit("text", "# Title");
        await flushPromises();
        expect(text.closings()).toBe(1);
        expect(sheets(text.wrapper)).toHaveLength(1);
        expect(text.wrapper.findComponent(IonTextarea).props("modelValue")).toBe("# Title");

        const file = await withTool();
        sheets(file.wrapper)[0].vm.$emit("attach", { path: "/p", name: "x.pdf", mime: "application/pdf", size: 1 });
        await flushPromises();
        expect(file.closings()).toBe(1);
        expect(sheets(file.wrapper)).toHaveLength(1);
        expect(file.wrapper.find("[data-test='staged']").exists()).toBe(true);
      });

      it("hides the game room at once with ✕, with the chat's own composer and calls back, and lets the game go once it has said goodbye", async () => {
        const { wrapper, says, closings } = await withGame();
        await wrapper.find("[data-test='close-game']").trigger("click");
        await flushPromises();
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find("[data-test='game-room']"))).toBe(true);
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
        expect(wrapper.find("[data-test='game-strip']").exists()).toBe(false);
        expect(wrapper.find(`[aria-label='Attach']`).exists()).toBe(true);
        expect(wrapper.find(`[aria-label='Video call']`).exists()).toBe(true);
        expect(sheets(wrapper)).toHaveLength(1);

        await says({ type: "ft.closed" });
        expect(wrapper.find("[data-test='game-room']").exists()).toBe(false);
        expect(sheets(wrapper)).toHaveLength(0);
      });

      it("goes through the game with the back button", async () => {
        const { wrapper, says, closings } = await withGame();
        back.handler?.();
        await flushPromises();
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find("[data-test='game-room']"))).toBe(true);
        await says({ type: "ft.closed" });
        expect(wrapper.find("[data-test='game-room']").exists()).toBe(false);
      });

      it("opens another plugin once the one on screen has said goodbye", async () => {
        const { wrapper, says, closings } = await withGame();
        await openTool(wrapper);
        expect(closings()).toBe(1);
        expect(wrapper.find(".ft-app").exists()).toBe(false);
        expect(sheets(wrapper)).toHaveLength(1);

        await says({ type: "ft.closed" });
        expect(wrapper.find("[data-test='game-room']").exists()).toBe(false);
        expect(sheets(wrapper)).toHaveLength(1);
        expect(sheets(wrapper)[0].props("plugin")).toMatchObject({ id: CODE.id });
        expect(wrapper.find(".ft-app").exists()).toBe(true);
        expect(hidden(wrapper.find(".ft-app"))).toBe(false);
      });

      it("opens a plugin chosen while another says goodbye once that one has gone", async () => {
        const { wrapper, says, closings } = await withGame();
        await wrapper.find("[data-test='close-game']").trigger("click");
        await openTool(wrapper);
        expect(closings()).toBe(1);
        expect(wrapper.find(".ft-app").exists()).toBe(false);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(1);
        expect(sheets(wrapper)[0].props("plugin")).toMatchObject({ id: CODE.id });
      });

      it("closes the window when the plugin opens the conversation it is in, and pushes no page", async () => {
        const { wrapper, says, closings } = await withTool({ opensChat: "c1" });
        await says({ type: "ft.openChat", id: "q1", ref: "ref_1" });
        expect(calls.some(([command]) => command === "core_plugin_open_chat")).toBe(true);
        expect(closings()).toBe(1);
        expect(hidden(wrapper.find(".ft-app"))).toBe(true);
        expect(push).not.toHaveBeenCalled();

        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
        expect(push).not.toHaveBeenCalled();
      });

      it("opens the other conversation the plugin asks for and keeps its window here", async () => {
        const { wrapper, says, closings } = await withTool({ opensChat: "c2" });
        await says({ type: "ft.openChat", id: "q1", ref: "ref_1" });
        expect(push).toHaveBeenCalledTimes(1);
        expect(push).toHaveBeenCalledWith("/chat/c2");
        expect(closings()).toBe(0);
        expect(hidden(wrapper.find(".ft-app"))).toBe(false);
        expect(sheets(wrapper)).toHaveLength(1);
      });

      it("says goodbye to the conversation it was opened in when another one is shown", async () => {
        bridge({ installed: [CODE, CHESS] });
        const wrapper = await thread({ play: CHESS.id, split: true });
        const { says, closings } = await up(wrapper);
        await wrapper.setProps({ chatId: "c2" });
        await flushPromises();
        expect(closings()).toBe(1);
        calls.length = 0;
        await says({ type: "ft.liveSend", id: "q1", data: "Ynll" });
        expect(calls).toContainEqual(["core_plugin_live_send", { plugin: CHESS.id, contact: "c1", data: "Ynll" }]);
        await says({ type: "ft.closed" });
        expect(sheets(wrapper)).toHaveLength(0);
      });
    });
  });
});

