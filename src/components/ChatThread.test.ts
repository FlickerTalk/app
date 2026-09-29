import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { IonTextarea } from "@ionic/vue";
import ChatThread from "./ChatThread.vue";
import MessageBubble from "./MessageBubble.vue";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { chat, store } from "../core";
import { refreshPlugins } from "../plugins";
import { defineComponent, h } from "vue";
import { startViewportFit } from "../viewport";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
// Android's back button: the handler the app listens with while something is open on top.
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => void (back.handler === handler && (back.handler = null)) };
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

  // The apps button follows what is installed. Adding or removing a tool in Settings has to show
  // up in a conversation that is already open, not only the next time it is entered.
  it("notices a tool added or removed while the conversation stays open", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    expect(wrapper.find("[data-test='apps']").exists()).toBe(true);

    // The user removes it from Settings, without leaving the conversation.
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_plugins" ? [] : undefined;
    });
    await refreshPlugins();
    await flushPromises();
    expect(wrapper.find("[data-test='apps']").exists()).toBe(false);
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

  // A2: a file a plugin made with the `propose` permission waits in the composer; the user sends
  // it, or throws it away. The plugin's window closes either way.
  it("stages what a plugin made and sends it only when the user says so", async () => {
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    let wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    await tapped(wrapper, "m4");
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);

    calls.length = 0;
    withPlugins([VIEWER], { core_read_message_file: new Error("that file is too big for a plugin") });
    wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
    await flushPromises();
    await tapped(wrapper, "m4");
    expect(wrapper.findComponent({ name: "PluginSheet" }).exists()).toBe(false);
    expect(calls).toContainEqual(["core_open_file", { message: "m4" }]);
  });

  // A5, as WhatsApp does it (2026-09-28): a stranger who wrote first is answered from the
  // conversation, where the yes and the no stand apart and blocking asks once; not from the list.
  describe("with someone who wrote first", () => {
    beforeEach(() => {
      store.requests = [{ ...store.chats[1], id: "ft_stranger", name: "Mamá", unread: 1, preview: "hi" }];
    });

    it("shows who it is instead of the composer, and no way to call yet", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs: { IonIcon: true } } });
      await flushPromises();
      const panel = wrapper.find("[data-test='request-panel']");
      expect(panel.exists()).toBe(true);
      expect(panel.text()).toContain("Mamá");
      expect(wrapper.findComponent(IonTextarea).exists()).toBe(false);
      expect(wrapper.find(`[aria-label='Voice call']`).exists()).toBe(false);
      expect(wrapper.find(`[aria-label='Video call']`).exists()).toBe(false);
    });

    it("blocks only after asking once", async () => {
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
      const wrapper = mount(ChatThread, { props: { chatId: "ft_stranger" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
    const wrapper = mount(ChatThread, { props: { chatId: "c1" }, shallow: false, global: { stubs: { IonIcon: true } } });
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
});
