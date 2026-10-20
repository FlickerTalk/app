import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { effectScope, nextTick, ref, watch } from "vue";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));
const toast = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

import { CATCH_UP, HOOK_LIMIT, useChatHooks, type HookRunner } from "./chatHooks";
import { installed, premiumLocked, type PluginHook, type PluginHookResult } from "./plugins";
import type { ChatHookState, ChatMessage, PluginView } from "./core";

// 2026-10-20 (plan of the translator §8): the chat's hooks in a conversation. A tool bound to them
// runs in a frame of its own out of sight, one run at a time, for five minutes at most; what it
// answers becomes a note under a message, a new draft, or the conversation's switches, which make
// it run by itself on the messages that arrive and the texts that go.
const TRANSLATOR: PluginView = {
  id: "com.flickertalk.translate",
  name: "Translator",
  version: "1.1.0",
  kind: "tool",
  asks: { network: [], messages: true, send: "propose", translate: true },
  granted: { network: [], messages: true, send: "propose", translate: true },
  installedAt: 1,
  hooks: {
    message: { label: "Translate", icon: "language-outline" },
    composer: { label: "Translate", icon: "language-outline" },
    chat: { label: "Chat language", icon: "language-outline" },
  },
};
const KEY = "k".repeat(43);

function text(id: string, words: string, mine = false, extra: Partial<ChatMessage> = {}): ChatMessage {
  return { id, mine, text: words, time: "09:00", sentAt: 1, status: "delivered", ...extra };
}

/** A frame that answers each run as `answer` says, and remembers what it was handed. */
function runner(answer: (hook: PluginHook) => Promise<PluginHookResult | null> | PluginHookResult | null = () => null) {
  const handed: PluginHook[] = [];
  const frame: HookRunner & { handed: PluginHook[] } = {
    handed,
    runHook: vi.fn(async (hook: PluginHook) => {
      handed.push(hook);
      return answer(hook);
    }),
  };
  return frame;
}

/** The conversation's hooks, with the frames mounted as the thread would mount them. */
function conversation(options: { messages?: ChatMessage[]; active?: boolean; flags?: ChatHookState[]; frame?: ReturnType<typeof runner> } = {}) {
  const contact = ref("ft_bob");
  const messages = ref<ChatMessage[]>(options.messages ?? []);
  const active = ref(options.active ?? true);
  const flags = options.flags ?? [];
  const frame = options.frame ?? runner();
  tauri.invoke.mockImplementation(async (command: string) => {
    if (command === "core_plugin_chat") return KEY;
    if (command === "core_plugin_ref") return "ref_1";
    if (command === "core_chat_hooks") return flags;
    return undefined;
  });
  const scope = effectScope();
  const hooks = scope.run(() => useChatHooks({ contact: () => contact.value, messages: () => messages.value, active: () => active.value }))!;
  // The thread mounts a frame for each plugin the hooks ask for, and hands it over.
  scope.run(() => watch(() => [...hooks.frames.value], (ids) => ids.forEach((id) => hooks.attach(id, frame)), { immediate: true }));
  return { hooks, contact, messages, active, frame, scope };
}

const invoked = (command: string) => tauri.invoke.mock.calls.filter(([one]) => one === command).map(([, args]) => args);

describe("the chat's hooks", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    toast.create.mockReset().mockImplementation(async () => ({ present: toast.present }));
    installed.value = [TRANSLATOR];
    premiumLocked.value = false;
  });
  afterEach(() => {
    vi.useRealTimers();
    installed.value = [];
  });

  it("runs a message hook out of sight with the chat's key and keeps its answer as a note", async () => {
    const frame = runner(() => ({ text: "nos vemos a las seis" }));
    const { hooks } = conversation({ frame });
    await flushPromises();
    const noting = hooks.noteMessage(TRANSLATOR, text("m1", "see you at six"));
    expect(hooks.noting.get("m1"), "the bubble says the tool is on it").toBe(TRANSLATOR.id);
    expect(await noting).toBe(true);
    expect(hooks.noting.has("m1")).toBe(false);
    expect(hooks.frames.value).toEqual([TRANSLATOR.id]);
    expect(frame.handed).toEqual([{ name: "message", text: "see you at six", chat: KEY, ref: "ref_1", lang: "en", manual: true }]);
    expect(invoked("core_message_note_set")).toEqual([{ message: "m1", plugin: TRANSLATOR.id, text: "nos vemos a las seis" }]);
  });

  it("notes nothing, and says nothing, when the tool answers nothing", async () => {
    const { hooks } = conversation({ frame: runner(() => null) });
    await flushPromises();
    expect(await hooks.noteMessage(TRANSLATOR, text("m1", "hola"))).toBe(false);
    expect(invoked("core_message_note_set")).toEqual([]);
    expect(toast.create).not.toHaveBeenCalled();
  });

  it("runs one hook at a time for each tool, in the order asked", async () => {
    const release: (() => void)[] = [];
    const frame = runner((hook) => new Promise((resolve) => release.push(() => resolve({ text: hook.text.toUpperCase() }))));
    const { hooks } = conversation({ frame });
    await flushPromises();
    const first = hooks.noteMessage(TRANSLATOR, text("m1", "one"));
    const second = hooks.noteMessage(TRANSLATOR, text("m2", "two"));
    await flushPromises();
    expect(frame.handed.map((hook) => hook.text)).toEqual(["one"]);
    release[0]();
    await first;
    await flushPromises();
    expect(frame.handed.map((hook) => hook.text)).toEqual(["one", "two"]);
    release[1]();
    await second;
    expect(invoked("core_message_note_set").map((args) => (args as { text: string }).text)).toEqual(["ONE", "TWO"]);
  });

  it("gives up on a tool silent for five minutes: nothing is kept, the user is told, its frame goes", async () => {
    vi.useFakeTimers();
    const { hooks } = conversation({ frame: runner(() => new Promise(() => {})) });
    await vi.advanceTimersByTimeAsync(0);
    let noted: boolean | undefined;
    void hooks.noteMessage(TRANSLATOR, text("m1", "hola")).then((said) => (noted = said));
    await vi.advanceTimersByTimeAsync(HOOK_LIMIT - 1);
    expect(noted).toBeUndefined();
    await vi.advanceTimersByTimeAsync(1);
    expect(noted).toBe(false);
    expect(hooks.frames.value).toEqual([]);
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("Translator") }));
    expect(invoked("core_message_note_set")).toEqual([]);
  });

  it("hands back the composer hook's new draft, or nothing", async () => {
    const answers: (PluginHookResult | null)[] = [{ text: "hola" }, { text: "  " }, null];
    const frame = runner(() => answers.shift() ?? null);
    const { hooks } = conversation({ frame });
    await flushPromises();
    expect(await hooks.compose(TRANSLATOR, "hello")).toBe("hola");
    expect(frame.handed[0]).toEqual({ name: "composer", text: "hello", chat: KEY, ref: null, lang: "en", manual: true });
    expect(await hooks.compose(TRANSLATOR, "hello")).toBeNull();
    expect(await hooks.compose(TRANSLATOR, "hello")).toBeNull();
  });

  it("keeps the chat hook's switches for the conversation, and nothing when it answers nothing", async () => {
    const window = runner(() => ({ auto: { message: true } }));
    const { hooks } = conversation();
    await flushPromises();
    await hooks.chooseForChat(TRANSLATOR, window);
    expect(window.handed).toEqual([{ name: "chat", text: "", chat: KEY, ref: null, lang: "en", manual: true }]);
    expect(invoked("core_chat_hooks_set")).toEqual([{ contact: "ft_bob", plugin: TRANSLATOR.id, autoMessage: true, autoComposer: false, lang: null }]);
    await hooks.chooseForChat(TRANSLATOR, runner(() => null));
    await hooks.chooseForChat(TRANSLATOR, runner(() => ({ text: "not switches" })));
    expect(invoked("core_chat_hooks_set")).toHaveLength(1);
  });

  describe("by itself", () => {
    const ON = (autoMessage: boolean, autoComposer: boolean): ChatHookState[] => [{ plugin: TRANSLATOR.id, autoMessage, autoComposer }];

    it("translates what is sent with the conversation's switch on, and says so when it cannot", async () => {
      const answers: (PluginHookResult | null)[] = [{ text: "hola" }, { text: "hello" }, null];
      const frame = runner(() => answers.shift() ?? null);
      const { hooks } = conversation({ flags: ON(false, true), frame });
      await flushPromises();
      expect(await hooks.outgoing("hello")).toBe("hola");
      expect(frame.handed[0]).toMatchObject({ name: "composer", text: "hello", manual: false });
      expect(await hooks.outgoing("hello"), "unchanged: sent as it is, without a word").toBe("hello");
      expect(toast.create).not.toHaveBeenCalled();
      expect(await hooks.outgoing("hello"), "nothing: sent as written").toBe("hello");
      expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("Translator") }));
    });

    it("sends as written with the switch off, the tool locked or not allowed to read", async () => {
      const frame = runner(() => ({ text: "hola" }));
      const off = conversation({ flags: ON(true, false), frame });
      await flushPromises();
      expect(await off.hooks.outgoing("hello")).toBe("hello");
      off.scope.stop();

      const on = conversation({ flags: ON(false, true), frame });
      await flushPromises();
      premiumLocked.value = true;
      expect(await on.hooks.outgoing("hello")).toBe("hello");
      premiumLocked.value = false;
      installed.value = [{ ...TRANSLATOR, granted: { ...TRANSLATOR.granted, messages: false } }];
      expect(await on.hooks.outgoing("hello")).toBe("hello");
      expect(frame.handed).toEqual([]);
      expect(toast.create).not.toHaveBeenCalled();
    });

    it("translates the latest received texts without a note as the chat opens, the newest first", async () => {
      const received = Array.from({ length: CATCH_UP + 5 }, (_, index) => text(`r${index}`, `text ${index}`));
      const messages = [
        ...received,
        text("mine", "mine", true),
        text("photo", "photo.jpg", false, { kind: "file" }),
        text("gone", "", false, { deleted: true }),
        text("noted", "already", false, { notes: [{ plugin: TRANSLATOR.id, text: "ya" }] }),
      ];
      const frame = runner((hook) => ({ text: `[${hook.text}]` }));
      conversation({ messages, flags: ON(true, false), frame });
      await vi.waitFor(() => expect(frame.handed).toHaveLength(CATCH_UP));
      expect(frame.handed.map((hook) => hook.text)).toEqual(received.slice(-CATCH_UP).reverse().map((one) => one.text));
      expect(frame.handed.every((hook) => !hook.manual && hook.name === "message")).toBe(true);
      expect(invoked("core_message_note_set")).toHaveLength(CATCH_UP);
    });

    it("translates what arrives while the chat is open, only while it is seen, and each text once", async () => {
      const frame = runner(() => null);
      const { messages, active } = conversation({ messages: [text("r1", "first")], flags: ON(true, false), frame });
      await vi.waitFor(() => expect(frame.handed).toHaveLength(1));
      messages.value = [...messages.value, text("r2", "second")];
      await vi.waitFor(() => expect(frame.handed).toHaveLength(2));
      active.value = false;
      messages.value = [...messages.value, text("r3", "third")];
      await flushPromises();
      expect(frame.handed).toHaveLength(2);
      active.value = true;
      await vi.waitFor(() => expect(frame.handed.map((hook) => hook.text)).toEqual(["first", "second", "third"]));
      // Said again with other words, it is a new text.
      messages.value = messages.value.map((one) => (one.id === "r1" ? { ...one, text: "first, edited" } : one));
      await vi.waitFor(() => expect(frame.handed.map((hook) => hook.text)).toContain("first, edited"));
      expect(frame.handed).toHaveLength(4);
    });

    it("starts translating what arrived once its chat hook switches it on", async () => {
      const frame = runner(() => ({ text: "x" }));
      const { hooks } = conversation({ messages: [text("r1", "first")], frame });
      await flushPromises();
      expect(frame.handed).toEqual([]);
      tauri.invoke.mockImplementation(async (command: string) => {
        if (command === "core_plugin_chat") return KEY;
        if (command === "core_plugin_ref") return "ref_1";
        if (command === "core_chat_hooks") return ON(true, false);
        return undefined;
      });
      await hooks.chooseForChat(TRANSLATOR, runner(() => ({ auto: { message: true, composer: false } })));
      await vi.waitFor(() => expect(frame.handed.map((hook) => hook.text)).toEqual(["first"]));
    });
  });

  it("lets its frames go when another conversation is on screen", async () => {
    const { hooks, contact } = conversation({ frame: runner(() => ({ text: "x" })) });
    await flushPromises();
    await hooks.noteMessage(TRANSLATOR, text("m1", "hola"));
    expect(hooks.frames.value).toEqual([TRANSLATOR.id]);
    contact.value = "ft_carol";
    await nextTick();
    expect(hooks.frames.value).toEqual([]);
  });
});
