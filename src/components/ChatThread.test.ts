import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { IonButton, IonDatetime, IonSearchbar, IonSegment, IonSegmentButton, IonTextarea } from "@ionic/vue";
import source from "./ChatThread.vue?raw";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { chat, heardTyping, store, TYPING_EVERY, TYPING_FADE } from "../core";
import { offered, refreshPlugins } from "../plugins";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";
import { setLocale } from "../i18n";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
// Android's back button: the handler the app listens with while something is open on top.
// Each listener is its own, as Tauri's: letting one go late never takes a newer one.
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

/**
 * Ionic's overlays show their content only once the browser presents them, which happy-dom never
 * does; as Ionic's Vue testing guidance suggests, the sheet modal is stubbed: its content while open.
 */
const IonModalStub = defineComponent({
  name: "IonModal",
  props: { isOpen: Boolean, breakpoints: { type: Array, default: undefined }, initialBreakpoint: { type: Number, default: undefined } },
  emits: ["didDismiss", "didPresent"],
  setup(props, { slots }) {
    return () => (props.isOpen ? h("div", { "data-test": "apps-sheet" }, slots.default?.()) : null);
  },
});
const stubs = { IonIcon: true, IonModal: IonModalStub };

// Each thread goes when its test ends, as a page does: what it left open must not linger.
enableAutoUnmount(afterEach);


/** A visual viewport the test moves, as the on-screen keyboard would (2026-09-29). */
class FakeViewport extends EventTarget {
  height = 900;
  offsetTop = 0;
  move(height: number) {
    this.height = height;
    this.dispatchEvent(new Event("resize"));
  }
}
/** `ion-content` whose element hands out a scroller the test can read. */
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
  beforeEach(() => seed());

  it("shows every message of the conversation", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.findAllComponents(MessageBubble)).toHaveLength(fixture.chats[0].messages.length);
  });

  // One fixed «Today» used to sit above every thread, whatever day its messages were from (seen
  // on real phones, 2026-10-02): each day of the thread now starts with its own separator.
  describe("day separators", () => {
    afterEach(() => vi.useRealTimers());
    const days = (wrapper: ReturnType<typeof mount>) => wrapper.findAll(".ft-thread__day").map((one) => one.text());

    it("names the day of earlier messages and puts today's under «Today»", async () => {
      // The fixture's conversation is from 22 September 2026.
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

  it("loads the conversation from the core and marks it read", async () => {
    mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_messages", { contact: "c1", limit: 200 }]);
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
  });

  // A message arriving while the conversation is on screen has been seen: its sender learns it.
  it("marks newly arrived messages read while the conversation is open", async () => {
    mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    calls.length = 0;
    // The list's unread count may still be stale when the new message shows up.
    const shown = chat("c1");
    if (shown) shown.unread = 0;
    shown?.messages.push({ id: "new", mine: false, text: "still there?", time: "09:50", sentAt: Date.now() });
    await flushPromises();
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
  });

  // architecture#21: on Android the page stays mounted with the app behind the home screen, and a
  // message that woke the core was told to its sender as read, though nobody had seen it.
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

  // §62: the attach button sends the picked files straight to the contact.
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

  // Voice messages: with nothing written, the send button records instead.
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

  // Never a silent no: without a microphone (or a recorder in this WebView) the user is told.
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

  // 2026-10-05: while they write, the header says so instead of the connection, and lets it fade.
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
      // Earlier tests wrote in this conversation: the moment since the last "typing" is over.
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

  // Every button of the header says what it does, from the catalogue (§84): the contact's own
  // button read as the avatar's initial followed by the name (seen in the App Store screenshots).
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

  // With nothing written the round button records a voice message; with text, it sends.
  it("has a composer to attach, record and send", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Attach']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(true);
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hi");
    await flushPromises();
    expect(wrapper.find("[aria-label='Send']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(false);
  });

  // The «+» unfolds two ways to attach (Ioan, 2026-09-23): a photo or video, through the system's
  // sheet over the chat, or any other file, through the document picker. Neither goes through the
  // WebView's own file input.
  it("unfolds photo-or-video and file from the attach button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Attach photo or video']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_pick_files", { accept: "image/*,video/*" }]);
    await wrapper.find("[aria-label='Attach file']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_pick_files", { accept: "" }]);
  });

  // The third thing the «+» unfolds (Ioan, 2026-09-23): a photo taken right now with the phone's
  // camera app, sent like any picked file.
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

  // 2026-09-30: a phone with no camera (the iOS simulator) or that does not allow it says so,
  // instead of a button that does nothing.
  it("says so when no photo can be taken", async () => {
    const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    const answer = internals.invoke;
    internals.invoke = (command, args) => (command === "core_take_photo" ? Promise.reject(new Error("no camera on this device")) : answer(command, args));
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await wrapper.find("[aria-label='Take a photo']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toBe("Can't take a photo: the camera is not available or not allowed");
  });

  // Issue app#3: the plugins live behind the apps button of the header, and each one does its
  // thing inside its own window.
  it("opens a plugin from the apps button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(false);

    await wrapper.find("[data-test='apps']").trigger("click");
    await flushPromises();
    // Ionic's components are Stencil "scoped" elements: in happy-dom their text is only in the HTML.
    expect(wrapper.find("[data-test='apps-sheet']").html()).toContain("Code block");

    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
    // The way out is always there, with the name of the tool next to it.
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(true);
    expect(wrapper.find(".ft-app__name").text()).toBe("Code block");
  });

  // 2026-10-01 (§108): a plugin opened from a conversation of a hidden session is open in that
  // session; from the main list, in none.
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

  // The apps follow what is installed. Adding or removing a tool in Settings has to show up in a
  // conversation that is already open, not only the next time it is entered. The button stays with
  // nothing installed, because it also leads to the games, on an iPhone too (2026-10-03).
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
    // The user removes it from Settings, without leaving the conversation.
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

  // A2: a file a plugin made with the `propose` permission waits in the composer; the user sends
  // it, or throws it away. The plugin's window closes either way.
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

  // A4: a file that waited for a tap is asked for through the core.
  it("asks for a waiting file when its bubble says so", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    await flushPromises();
    wrapper.findAllComponents(MessageBubble)[0].vm.$emit("download", "m1");
    await flushPromises();
    expect(calls).toContainEqual(["core_accept_file", { message: "m1" }]);
  });

  // Issue app#4: the emoji live in the core, in the composer, not in a plugin.
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

  // A long press on a message opens what can be done with it (Ioan, 2026-09-23): fold it, send it
  // on, hand it to another app, or erase it here.
  // Real time, not fake: Vue stamps its listeners with the clock, and a handler attached under a
  // fake clock ignores a click that comes after (its own guard against events from a past patch).
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

  // 2026-09-27: "open with": a message goes to a plugin that opens its kind, with a way back.
  it("opens a message with a plugin that says it opens its kind", async () => {
    // The seed's bridge answers everything else (the messages, above all).
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

  // 2026-10-02 (plan of the catalogue's translations): a Spanish phone names a tool in Spanish in
  // the apps sheet, in "open with" and in its window, from its package.
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
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await wrapper.find("[data-test='apps']").trigger("click");
      expect(wrapper.find("[data-test='app-com.flickertalk.notes'] ion-label").element.innerHTML).toBe("Notas");
      await wrapper.find("[data-test='app-com.flickertalk.notes']").trigger("click");
      await flushPromises();
      expect(wrapper.find(".ft-app__name").text()).toBe("Notas");
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

  // Document viewer (2026-09-27): a tap on a file shows it in its viewer when there is one, and
  // goes to another app otherwise, or when the bytes cannot be handed over.
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

  // app#78 (Samsung, 2026-10-03): with a blocked contact, a message waited forever and nothing said
  // why. The composer gives way to a notice and a way to unblock them.
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

  // A5, as WhatsApp does it (2026-09-28): a stranger who wrote first is answered from the
  // conversation, where the yes and the no stand apart and blocking asks once; not from the list.
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
      // Nor the apps and games.
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
      // The core now lists it among the contacts, not the requests.
      store.chats.push(stranger);
      store.requests = [];
      await flushPromises();
      expect(wrapper.find("[data-test='request-panel']").exists()).toBe(false);
      expect(wrapper.findComponent(IonTextarea).exists()).toBe(true);
    });
  });

  // Found on the phones (2026-09-28): back with a plugin open left the chat, or the whole app on
  // the tablet. It closes what is open on top, and only that.
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
    // The text message: no file, no viewer, no «another app».
    await pressed(wrapper);
    expect(wrapper.find("[data-test='open-with']").exists()).toBe(false);
    await wrapper.find("[data-test='actions']").trigger("click");
    // The PDF: its viewer, and the way out to another app.
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

  // Erasing is for good and only here: it asks once before doing it.
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
    // Never to the conversation it is already in.
    expect(wrapper.find(`[data-test='to-c1']`).exists()).toBe(false);

    await wrapper.find(`[data-test='to-${other}']`).trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_forward", { message: id, contact: other }]);
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);
  });

  // 2026-10-06: a text for later: the clock by Send opens the time in Ionic's sheet, with Ionic's
  // date and time picker, and Schedule queues it.
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
    /** Opened and on screen, as Ionic says once its sheet has presented. */
    const presented = async (text: string) => {
      const wrapper = await opened(text);
      scheduleSheet(wrapper).vm.$emit("didPresent");
      await flushPromises();
      return wrapper;
    };

    // Seen on an iPhone 13 mini (2026-10-06): on the first open the day grid stayed invisible, since
    // the picker laid itself out inside a sheet not yet on screen. It is built once the sheet is up.
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

    // Seen on an iPhone 13 mini (2026-10-06): the sheet rose 197 px tall, without the picker, and
    // snapped to 555 px once it was built. The picker's room is kept from the start.
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

    // Seen on a Spanish iPhone (2026-10-06): the picker said «Time» and used the WebView's
    // language. It takes the app's language, and its time label comes from the catalogue.
    it("picks the time in the app's language", async () => {
      await setLocale("es");
      try {
        const wrapper = await presented("buenos días");
        const picker = wrapper.findComponent(IonDatetime);
        expect(picker.props("locale")).toBe("es");
        expect(picker.find("[slot='time-label']").text()).toBe("Hora");
      } finally {
        await setLocale("en");
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

  // 2026-10-05: a search in the conversation, on this phone: the hits list under the header and a
  // tap goes to the message. Since 2026-10-06 it opens from the contact page (`?search=1`), as in
  // Messenger and WhatsApp, not from a button of the header.
  describe("searching the conversation", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    const typed = async (wrapper: ReturnType<typeof mount>, text: string) => {
      wrapper.findComponent(IonSearchbar).vm.$emit("update:modelValue", text);
      // The query's watcher starts its wait on the next tick, as it did after the old field's input.
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

  // 2026-10-05: my own text can be said again with other words, and any message of mine taken
  // back for both sides; the sheet offers both only where they apply.
  describe("editing and taking back", () => {
    const pressOn = async (wrapper: ReturnType<typeof mount>, index: number) => {
      await wrapper.findAll("[data-test='bubble']")[index].trigger("pointerdown");
      await new Promise((wake) => setTimeout(wake, 550));
      await flushPromises();
    };

    it("puts my text in the composer and sends the new words as an edit", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
      await flushPromises();
      await pressOn(wrapper, 1); // m2: mine, a text
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
      await pressOn(wrapper, 0); // m1: theirs
      expect(wrapper.find("[data-test='edit']").exists()).toBe(false);
      await wrapper.find("[data-test='actions']").trigger("click");
      await pressOn(wrapper, 3); // m4: my file
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

    // 2026-10-06: a text for later that never went out has nobody to take it back from: the sheet
    // offers only to delete it here, which cancels it and leaves no mark.
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

  // 2026-10-05: pinned messages: the sheet pins and unpins; a strip under the header shows the
  // latest pinned, and a tap goes to it and moves on to the next.
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

  // 2026-10-05: an emoji on a message, from the sheet; the same one again takes it back.
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

  // 2026-10-06, the review of the chat features: Ionic's own controls, with an accessible name,
  // colours from Ionic's palette and logical sides, so that Arabic (right to left) works.
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
      await pressOn(wrapper, 1); // m2: mine, a text
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

  // 2026-10-05: answering quotes the message; the quote waits in the composer until sent or dropped.
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

    // The quote in a bubble names the contact as the reply bar does (2026-10-06).
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

  // Seen in Arabic (2026-10-02): a contact's name reads in its own direction, not in the app's.
  it("offers to forward to contacts whose names read in their own direction", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs } });
    await flushPromises();
    await pressed(wrapper);
    await wrapper.find("[data-test='forward']").trigger("click");
    expect(wrapper.find(`[data-test='to-${fixture.chats[1].id}']`).attributes("dir")).toBe("auto");
  });

  // The keyboard shrinks the conversation: the last message stays in sight above the composer.
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

    // Once it is gone, it no longer moves anything.
    wrapper.unmount();
    scroller.scrollTop = 1200;
    viewport.move(900);
    viewport.move(560);
    expect(scroller.scrollTop).toBe(1200);
    stop();
    document.documentElement.removeAttribute("style");
  });

  // Plan 10 (app 1.3.0): games are plugins of their own section, played inside a conversation.
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

    /** The bridge: what is installed, what the catalogue offers, and the messages of the chat. */
    function bridge({ installed = [] as unknown[], catalogue = [] as unknown[], messages = [] as unknown[], addFails = false } = {}) {
      let list = installed as Array<Record<string, unknown>>;
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_plugins") return list;
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
    }

    const thread = async (props: Record<string, unknown> = {}) => {
      const wrapper = mount(ChatThread, { props: { chatId: "c1", ...props }, shallow: false, global: { stubs } });
      await flushPromises();
      return wrapper;
    };
    /** The apps button, then the Games tab of its sheet. */
    /** The apps button, then the Games segment of its sheet. */
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

    // Ioan, 2026-10-02: one button, the apps, in the header (three buttons, so a name has room on
    // a small phone). It opens an Ionic sheet modal, as the apps themselves open, with a segment for
    // the plugins and one for the games.
    it("opens a sheet with a segment for the plugins and one for the games", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread();
      expect(wrapper.find("[data-test='games']").exists()).toBe(false);
      // Voice, video and the apps; the search lives on the contact page (Ioan, 2026-10-06).
      expect(endButtons(wrapper)).toHaveLength(3);
      expect(wrapper.find("[data-test='search']").exists()).toBe(false);
      expect(wrapper.find("[data-test='apps-sheet']").exists()).toBe(false);
      await wrapper.find("[data-test='apps']").trigger("click");
      // A sheet: it rises from the bottom and has heights to be dragged between; and a name.
      const sheet = wrapper.findComponent(IonModalStub);
      expect(sheet.attributes("aria-label")).toBe("Plugins");
      expect(sheet.props("isOpen")).toBe(true);
      expect(sheet.props("breakpoints")).toContain(sheet.props("initialBreakpoint"));
      const segments = wrapper.findAllComponents(IonSegmentButton);
      // Ionic's components are Stencil "scoped" elements: in happy-dom their text is only in the HTML.
      expect(segments.map((one) => [one.props("value"), one.find("ion-label").element.innerHTML])).toEqual([
        ["tools", "Plugins"],
        ["games", "Games"],
      ]);
      // With a tool installed it opens on the plugins, and they are tools only.
      expect(selected(wrapper)).toBe("tools");
      expect(wrapper.find(`[data-test='app-${CODE.id}']`).exists()).toBe(true);
      expect(wrapper.find(`[data-test='app-${CHESS.id}']`).exists()).toBe(false);
      // The games segment: games only.
      wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
      await flushPromises();
      expect(selected(wrapper)).toBe("games");
      expect(wrapper.find("[data-test='games-sheet']").html()).toContain("Chess");
      expect(wrapper.find(`[data-test='app-${CODE.id}']`).exists()).toBe(false);
    });

    // Shown wherever games can be had, so they can be found even with nothing installed.
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
      // Dismissed by Ionic: dragged down, a tap on the backdrop, or Escape.
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

    // 2026-10-03: the games travel inside the app, so an iPhone, which downloads nothing (App
    // Store 4.7, §52), has the games segment, and the button with nothing installed, like any phone.
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
      expect(push).toHaveBeenCalledWith("/tabs/games");
    });

    it("says there is no game yet, and still leads to more", async () => {
      bridge({ installed: [CODE] });
      const wrapper = await thread();
      await openGames(wrapper);
      expect(wrapper.find("[data-test='games-sheet']").html()).toContain("No games yet");
      expect(wrapper.find("[data-test='more-games-link']").exists()).toBe(true);
    });

    // Plan 10.5: the game opens in the conversation, with the live channel to the other phone.
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

    // Plan decision 11: the first time, one sheet; refused, the game does not open.
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

    // From the games tab: `/chat/<contact>?play=<id>` opens the game in that conversation.
    it("opens the game the address asks for", async () => {
      bridge({ installed: [CODE, CHESS] });
      const wrapper = await thread({ play: CHESS.id });
      const sheet = wrapper.findComponent({ name: "PluginSheet" });
      expect(sheet.props("plugin")).toMatchObject({ id: CHESS.id });
      expect(sheet.props("live")).toBe(true);
    });

    it("asks first when the game the address asks for lacks what it needs, and never opens a tool", async () => {
      bridge({ installed: [CODE, UNGRANTED] });
      const asked = await thread({ play: CHESS.id });
      expect(asked.find("[data-test='game-permissions']").exists()).toBe(true);
      expect(asked.findComponent({ name: "PluginSheet" }).exists()).toBe(false);

      const tool = await thread({ play: CODE.id });
      expect(tool.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    });

    // Plan 10.6: the invitation is a text in the composer, with the game's page; the user sends it.
    it("leaves an invitation in the composer, without sending it", async () => {
      bridge({ installed: [CHESS] });
      const wrapper = await thread();
      await openGames(wrapper);
      await wrapper.find(`[data-test='invite-${CHESS.id}']`).trigger("click");
      await flushPromises();
      expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("🎮 Chess · Shall we play? https://flickertalk.com/games/chess");
      expect(wrapper.find("[data-test='games-sheet']").exists()).toBe(false);
      expect(calls.map(([command]) => command)).not.toContain("core_send");
    });

    const INVITE = { id: "g1", outgoing: false, text: "🎮 Shall we play Go? https://flickertalk.com/games/go", sentAt: 0, state: "delivered" };

    // 2026-10-02 (plan of the catalogue's translations): a Spanish phone names a game in Spanish in
    // the games sheet, in what it asks first, in its room and in the invitation the user sends. The
    // package installed here has no translation: the catalogue's entry has.
    it("names a game in the phone's language wherever it is shown", async () => {
      await setLocale("es");
      try {
        bridge({ installed: [UNGRANTED] });
        offered.value = [{ ...GO, id: CHESS.id, name: "Chess", installed: true, locales: { es: { name: "Ajedrez" } } } as never];
        const wrapper = await thread();
        await openGames(wrapper);
        expect(wrapper.find(`[data-test='game-${CHESS.id}'] ion-label`).element.innerHTML).toBe("Ajedrez");
        await wrapper.find(`[data-test='game-${CHESS.id}']`).trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='game-permissions']").text()).toContain("Ajedrez");
        await wrapper.find("[data-test='game-allow']").trigger("click");
        await flushPromises();
        expect(wrapper.find("[data-test='game-room']").attributes("aria-label")).toBe("Ajedrez");
        expect(wrapper.find("[data-test='game-bar']").text()).toContain("Ajedrez");
        expect(wrapper.findComponent({ name: "PluginSheet" }).props("plugin")).toMatchObject({ name: "Ajedrez" });
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

    // Plan 10.6: a game not here yet is installed from the signed catalogue, granted and opened.
    it("installs, grants and opens the game an invitation is for", async () => {
      bridge({ installed: [CODE], catalogue: [GO], messages: [INVITE] });
      const wrapper = await thread();
      // The catalogue is read because there is an invitation, and handed to the bubbles.
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

    // Opening a chat does not reach the catalogue; only an invitation to an unknown game does.
    it("does not read the catalogue for a conversation without invitations", async () => {
      bridge({ installed: [CHESS], messages: [{ ...INVITE, text: "https://flickertalk.com/games/chess" }] });
      await thread();
      bridge({ installed: [CHESS], messages: [{ ...INVITE, text: "see you at six" }] });
      await thread();
      expect(calls.map(([command]) => command)).not.toContain("core_catalogue");
    });

    // Ioan, 2026-10-02 (option A): a game is played inside the conversation, so the two can write
    // while playing. It takes the place of the messages; the header and the composer stay.
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
        // Not the full-screen window of a tool.
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

      // Plan finding 4: with the composer in sight, what a game says waits there and the game goes on.
      it("keeps the game open when it proposes a text or a file", async () => {
        const wrapper = await playChess();
        const sheet = wrapper.findComponent({ name: "PluginSheet" });
        sheet.vm.$emit("text", "♟️ Chess: I won");
        await flushPromises();
        expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("♟️ Chess: I won");
        expect(room(wrapper).exists()).toBe(true);

        // A file it made: staged, and the `done` that follows it does not close the game.
        sheet.vm.$emit("attach", { path: "/p/game.pgn", name: "game.pgn", mime: "application/x-chess-pgn", size: 9 });
        sheet.vm.$emit("done");
        await flushPromises();
        expect(wrapper.find("[data-test='staged']").exists()).toBe(true);
        expect(room(wrapper).exists()).toBe(true);

        // The game asking to close itself still closes it.
        wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("done");
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
      });

      // Ioan: no video while playing; and no files from the room.
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
        // Nothing new yet: the strip says what a tap does.
        expect(strip().find("ion-label").element.innerHTML).toBe("Show the chat");
        arrives("nice move!");
        await flushPromises();
        expect(strip().html()).toContain("nice move!");

        await wrapper.find("[data-test='game-peek']").trigger("click");
        await flushPromises();
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
        expect(hidden(wrapper.find("[data-test='game-area']"))).toBe(true);
        expect(room(wrapper).findComponent({ name: "PluginSheet" }).exists()).toBe(true);

        // Back to the game: what was read is not shown again.
        await wrapper.find("[data-test='game-peek']").trigger("click");
        await flushPromises();
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(true);
        expect(hidden(wrapper.find("[data-test='game-area']"))).toBe(false);
        expect(strip().html()).not.toContain("nice move!");
      });

      // Seen in Arabic (2026-10-02): the line the other one wrote reads in its own direction.
      it("shows the other one's line in its own direction", async () => {
        const wrapper = await playChess();
        arrives("¿otra?");
        await flushPromises();
        expect(wrapper.find("[data-test='game-strip'] ion-label").attributes("dir")).toBe("auto");
      });

      // §84: read only once it could be read: in the strip, the latest of what arrived; when the
      // thread is shown, all of it.
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

      it("closes the game with the back button and stays in the chat", async () => {
        const wrapper = await playChess();
        expect(back.handler).not.toBeNull();
        back.handler?.();
        await flushPromises();
        expect(room(wrapper).exists()).toBe(false);
        expect(hidden(wrapper.find(".ft-thread__content"))).toBe(false);
      });
    });

    // 2026-10-02 (seen on two phones and the iOS simulator): closed by the app, a plugin in a live
    // session vanished without a word and the other side kept saying both were there. Every way of
    // closing it now goes through the sheet's `close()`: the window goes at once, the plugin is told
    // and kept out of sight until it has said goodbye (or a few tenths of a second), and then it goes.
    describe("closing a plugin", () => {
      type Thread = Awaited<ReturnType<typeof thread>>;
      const sheets = (wrapper: Thread) => wrapper.findAllComponents({ name: "PluginSheet" });
      const hidden = (element: { attributes: (name: string) => string | undefined }) => /display:\s*none/.test(element.attributes("style") ?? "");

      /** The frame of the plugin on screen, up (`ft.ready`): what the app tells it, and its answers. */
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

      async function withTool() {
        bridge({ installed: [CODE, CHESS] });
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

      // One press closes the plugin, and only that: while it says goodbye, Back stays taken, so
      // another press neither closes it again nor falls through to leave the chat.
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
        // Gone, Back is the system's again.
        expect(back.handler).toBeNull();
        expect(push).not.toHaveBeenCalled();
      });

      // A page kept mounted under another one (`active` false) does not hold Back while its plugin
      // says goodbye; back on screen before the plugin has gone, it holds it again.
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

      // Upstream (2026-10-02): a plugin left open in a page that goes under another stays open, for
      // the user to come back to. Going under is not a way of closing it.
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

      // A chat page under another reads the address of the page on top (`ft.openChat` to another
      // conversation, say): its `chatId` changes for a while without anyone leaving it. Its plugin
      // stays, and still belongs to its own conversation.
      // The address changes before Ionic says the page is leaving: `active` is still true then.
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

      // 2026-10-02: the page going back closes what is open here through the same way out, so the
      // plugin's goodbye goes out during Ionic's transition.
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

      // Back closed the game, and the page goes back during the goodbye: one goodbye.
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

      // ✕, then Back while the plugin says goodbye: one goodbye, and Back does not leave the chat.
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

      // Back during a goodbye with another plugin chosen meanwhile: that one, once open, has Back.
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

      // Another plugin opened over the one on screen (a tool from the game room): the first says
      // goodbye, and the second opens with a frame of its own once it has.
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

      // The same, while the first is still saying goodbye.
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

      // The split view shows another conversation: the plugin says goodbye to the one it was opened
      // in, never to the one now on screen.
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

