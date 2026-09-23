import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonTextarea } from "@ionic/vue";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import { calls, fixture, seed } from "../__tests__/seed";
import { chat } from "../core";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
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

describe("ChatThread", () => {
  beforeEach(() => seed());

  it("shows every message of the conversation", () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.findAllComponents(MessageBubble)).toHaveLength(fixture.chats[0].messages.length);
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
    shown?.messages.push({ id: "new", mine: false, text: "still there?", time: "09:50" });
    await flushPromises();
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
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

  // With nothing written the round button records a voice message; with text, it sends.
  it("has a composer to attach, record and send", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: true });
    expect(wrapper.find("[aria-label='Attach file']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(true);
    wrapper.findComponent(IonTextarea).vm.$emit("update:modelValue", "hi");
    await flushPromises();
    expect(wrapper.find("[aria-label='Send']").exists()).toBe(true);
    expect(wrapper.find("[aria-label='Record voice message']").exists()).toBe(false);
  });

  // Issue app#3: the plugins live behind the apps button of the header, and each one does its
  // thing inside its own window.
  it("opens a plugin from the apps button", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(false);

    await wrapper.find("[data-test='apps']").trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("Code block");

    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(true);
    // The way out is always there, with the name of the tool next to it.
    expect(wrapper.find("[data-test='close-app']").exists()).toBe(true);
    expect(wrapper.find(".ft-app__name").text()).toBe("Code block");
  });

  it("puts in the composer the text a plugin proposes", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    await wrapper.find("[data-test='apps']").trigger("click");
    await wrapper.find("[data-test='app-com.flickertalk.code']").trigger("click");
    await flushPromises();

    wrapper.findComponent({ name: "PluginSheet" }).vm.$emit("text", "# Title");
    await flushPromises();
    expect(wrapper.findComponent(IonTextarea).props("modelValue")).toBe("# Title");
  });

  // Issue app#4: the emoji live in the core, in the composer, not in a plugin.
  it("puts the emoji that was picked at the end of what is being written", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    expect(wrapper.find("[data-test='actions']").exists()).toBe(false);

    await pressed(wrapper);
    const actions = wrapper.find("[data-test='actions']");
    expect(actions.exists()).toBe(true);
    for (const what of ["fold", "forward", "share", "delete"]) {
      expect(actions.find(`[data-test='${what}']`).exists()).toBe(true);
    }
  });

  it("folds and unfolds the message it was asked about", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    await pressed(wrapper);
    const id = fixture.chats[0].messages[0].id;

    await wrapper.find("[data-test='share']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_share_message", { message: id }]);
  });

  // Erasing is for good and only here: it asks once before doing it.
  it("erases a message only after asking", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
});
