import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { effectScope, nextTick, ref, watch } from "vue";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));
const toast = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
/** The app's own alert for a tool's question: the role the user's tap dismissed it with. */
const alert = vi.hoisted(() => ({ create: vi.fn(), role: "confirm" as string }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
  alertController: { create: alert.create },
}));

import { CATCH_UP, HOOK_LIMIT, useChatHooks, type HookRunner } from "./chatHooks";
import { installed, premiumLocked, type PluginHook, type PluginHookResult } from "./plugins";
import { setLocale } from "./i18n";
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
    settings: {
      label: "Translation",
      icon: "language-outline",
      fields: [
        { key: "lang", type: "select", label: "Language" },
        { key: "receive", type: "toggle", label: "Translate what I receive" },
        { key: "send", type: "toggle", label: "Send translated" },
      ],
      note: "Language packs are downloaded once; translation happens on the phone.",
    },
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
function conversation(options: { messages?: ChatMessage[]; active?: boolean; flags?: ChatHookState[]; frame?: ReturnType<typeof runner>; sample?: string } = {}) {
  const contact = ref("ft_bob");
  const messages = ref<ChatMessage[]>(options.messages ?? []);
  const active = ref(options.active ?? true);
  const flags = options.flags ?? [];
  const frame = options.frame ?? runner();
  tauri.invoke.mockImplementation(async (command: string) => {
    if (command === "core_plugin_chat") return KEY;
    if (command === "core_plugin_ref") return "ref_1";
    if (command === "core_chat_hooks") return flags;
    // The last texts received, one per line, as the core hands them to a tool allowed to read them.
    if (command === "core_hook_sample") return options.sample ?? "";
    return undefined;
  });
  const scope = effectScope();
  const hooks = scope.run(() => useChatHooks({ conversation: () => contact.value, messages: () => messages.value, active: () => active.value }))!;
  // The thread mounts a frame for each plugin the hooks ask for, and hands it over.
  scope.run(() => watch(() => [...hooks.frames.value], (ids) => ids.forEach((id) => hooks.attach(id, frame)), { immediate: true }));
  return { hooks, contact, messages, active, frame, scope };
}

const invoked = (command: string) => tauri.invoke.mock.calls.filter(([one]) => one === command).map(([, args]) => args);

describe("the chat's hooks", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    toast.create.mockReset().mockImplementation(async () => ({ present: toast.present }));
    alert.role = "confirm";
    alert.create.mockReset().mockImplementation(async () => ({ present: async () => {}, onDidDismiss: async () => ({ role: alert.role }) }));
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

  // §8.2c: a language pack missing, the tool asks in its answer instead of opening its window; the
  // app asks in its own alert and, on a yes, runs the hook again with the answer.
  it("asks the tool's question in the app's alert and runs the hook again on a yes", async () => {
    const frame = runner((hook) => (hook.answer ? { text: "hola" } : { ask: { text: "Download French? About 30 MB, once." } }));
    const { hooks } = conversation({ frame });
    await flushPromises();
    expect(await hooks.noteMessage(TRANSLATOR, text("m1", "salut"))).toBe(true);
    expect(alert.create).toHaveBeenCalledTimes(1);
    const asked = alert.create.mock.calls[0][0] as { message: string; buttons: { text: string; role?: string }[] };
    expect(asked.message).toBe("Download French? About 30 MB, once.");
    expect(asked.buttons.map((one) => [one.text, one.role])).toEqual([
      ["Cancel", "cancel"],
      ["OK", "confirm"],
    ]);
    expect(frame.handed.map((hook) => hook.answer)).toEqual([undefined, true]);
    expect(frame.handed[1]).toMatchObject({ name: "message", text: "salut", manual: true });
    expect(invoked("core_message_note_set")).toEqual([{ message: "m1", plugin: TRANSLATOR.id, text: "hola" }]);
  });

  it("keeps nothing when the user says no to the tool's question", async () => {
    alert.role = "cancel";
    const frame = runner(() => ({ ask: { text: "Download French?" } }));
    const { hooks } = conversation({ frame });
    await flushPromises();
    expect(await hooks.noteMessage(TRANSLATOR, text("m1", "salut"))).toBe(false);
    expect(frame.handed).toHaveLength(1);
    expect(invoked("core_message_note_set")).toEqual([]);
  });

  it("says when the tool fails, and keeps nothing", async () => {
    const frame = runner(() => {
      throw new Error("gone");
    });
    const { hooks } = conversation({ frame });
    await flushPromises();
    expect(await hooks.noteMessage(TRANSLATOR, text("m1", "salut"))).toBe(false);
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "Translator did not answer" }));
    expect(invoked("core_message_note_set")).toEqual([]);
  });

  // §8.2d: the link under a received text is there while the conversation's language, as the tool
  // found it, is not the app's, or is not known yet; never without a tool bound to messages.
  describe("the conversation's language", () => {
    const WITH = (lang?: string): ChatHookState[] => [{ plugin: TRANSLATOR.id, autoMessage: false, autoComposer: false, ...(lang === undefined ? {} : { lang }) }];
    afterEach(async () => setLocale("en"));

    it("offers the message hook while the conversation's language is another, or not known", async () => {
      const french = conversation({ flags: WITH("fr") });
      await flushPromises();
      expect(french.hooks.messageTool.value?.id).toBe(TRANSLATOR.id);
      expect(french.hooks.language.value).toBe("fr");
      expect(french.hooks.languageName.value).toBe("French");
      expect(french.hooks.offersHook.value).toBe(true);
      french.scope.stop();

      const unknown = conversation({ flags: [] });
      await flushPromises();
      expect(unknown.hooks.language.value).toBeUndefined();
      expect(unknown.hooks.offersHook.value).toBe(true);
      unknown.scope.stop();

      for (const same of ["en", "en-GB"]) {
        const mine = conversation({ flags: WITH(same) });
        await flushPromises();
        expect(mine.hooks.offersHook.value, same).toBe(false);
        mine.scope.stop();
      }
    });

    it("names the language in the app's language", async () => {
      await setLocale("es");
      const { hooks } = conversation({ flags: WITH("fr") });
      await flushPromises();
      expect(hooks.languageName.value).toBe("francés");
    });

    it("offers nothing without a tool bound to messages, or with only a game", async () => {
      installed.value = [{ ...TRANSLATOR, hooks: { composer: TRANSLATOR.hooks!.composer } }];
      const { hooks } = conversation({ flags: WITH("fr") });
      await flushPromises();
      expect(hooks.offersHook.value).toBe(false);
      installed.value = [{ ...TRANSLATOR, kind: "game" }];
      expect(hooks.offersHook.value).toBe(false);
    });

    it("says it translates by itself, and into what, only with a switch on and the language known", async () => {
      const on = conversation({ flags: [{ plugin: TRANSLATOR.id, autoMessage: true, autoComposer: false, lang: "fr" }] });
      await flushPromises();
      expect(on.hooks.translating.value).toBe("French");
      on.scope.stop();
      const off = conversation({ flags: WITH("fr") });
      await flushPromises();
      expect(off.hooks.translating.value).toBe("");
      off.scope.stop();
      const none = conversation({ flags: [{ plugin: TRANSLATOR.id, autoMessage: true, autoComposer: true, lang: "" }] });
      await flushPromises();
      expect(none.hooks.translating.value, "no language: None").toBe("");
    });
  });

  describe("by itself", () => {
    const ON = (autoMessage: boolean, autoComposer: boolean): ChatHookState[] => [{ plugin: TRANSLATOR.id, autoMessage, autoComposer }];

    it("translates what is sent with the conversation's switch on, with the user's words to keep, and says so when it cannot", async () => {
      const answers: (PluginHookResult | null)[] = [{ text: "hola" }, { text: "hello" }, null];
      const frame = runner(() => answers.shift() ?? null);
      const { hooks } = conversation({ flags: ON(false, true), frame });
      await flushPromises();
      expect(await hooks.outgoing("hello")).toEqual({ text: "hola", original: { plugin: TRANSLATOR.id, text: "hello" } });
      expect(frame.handed[0]).toMatchObject({ name: "composer", text: "hello", manual: false });
      expect(await hooks.outgoing("hello"), "unchanged: sent as it is, without a word").toEqual({ text: "hello" });
      expect(toast.create).not.toHaveBeenCalled();
      expect(await hooks.outgoing("hello"), "nothing: sent as written").toEqual({ text: "hello" });
      expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("Translator") }));
    });

    it("keeps the user's words under the message that went", async () => {
      const { hooks } = conversation({ flags: ON(false, true) });
      await flushPromises();
      await hooks.keepOriginal("sent1", { plugin: TRANSLATOR.id, text: "hello" });
      expect(invoked("core_message_note_set")).toEqual([{ message: "sent1", plugin: TRANSLATOR.id, text: "hello" }]);
      await hooks.keepOriginal(undefined, { plugin: TRANSLATOR.id, text: "hello" });
      expect(invoked("core_message_note_set"), "no message, no note").toHaveLength(1);
    });

    it("does not ask the user anything on a run of its own", async () => {
      const frame = runner(() => ({ ask: { text: "Download French?" } }));
      conversation({ messages: [text("r1", "salut")], flags: ON(true, false), frame });
      await vi.waitFor(() => expect(frame.handed).toHaveLength(1));
      await flushPromises();
      expect(alert.create).not.toHaveBeenCalled();
      expect(invoked("core_message_note_set")).toEqual([]);
    });

    it("sends as written with the switch off, the tool locked or not allowed to read", async () => {
      const frame = runner(() => ({ text: "hola" }));
      const off = conversation({ flags: ON(true, false), frame });
      await flushPromises();
      expect(await off.hooks.outgoing("hello")).toEqual({ text: "hello" });
      off.scope.stop();

      const on = conversation({ flags: ON(false, true), frame });
      await flushPromises();
      premiumLocked.value = true;
      expect(await on.hooks.outgoing("hello")).toEqual({ text: "hello" });
      premiumLocked.value = false;
      installed.value = [{ ...TRANSLATOR, granted: { ...TRANSLATOR.granted, messages: false } }];
      expect(await on.hooks.outgoing("hello")).toEqual({ text: "hello" });
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

    it("starts translating what arrived once its switch is on", async () => {
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
      await hooks.refresh();
      await vi.waitFor(() => expect(frame.handed.map((hook) => hook.text)).toEqual(["first"]));
    });
  });

  // §8.2d: the settings hook draws the conversation's group on the contact's page, and finds the
  // conversation's language as a conversation without one opens. The app keeps only the switches
  // and the language it answers, for the hooks the tool binds to.
  describe("the settings hook", () => {
    const VALUES = { lang: "fr", receive: true, send: false };
    const ANSWER = { values: VALUES, options: { lang: [{ value: "", label: "None" }, { value: "fr", label: "French (detected)" }] }, auto: { message: true, composer: false }, lang: "fr" };

    it("runs it with the values changed, and keeps the switches and the language it answers", async () => {
      const frame = runner(() => ANSWER);
      const { hooks } = conversation({ frame });
      await flushPromises();
      expect(await hooks.settings(TRANSLATOR, { manual: true, values: VALUES })).toEqual(ANSWER);
      expect(frame.handed).toEqual([{ name: "settings", text: "", chat: KEY, ref: null, lang: "en", manual: true, values: VALUES }]);
      expect(invoked("core_chat_hooks_set")).toEqual([{ conversation: "ft_bob", plugin: TRANSLATOR.id, autoMessage: true, autoComposer: false, lang: "fr" }]);
      expect(invoked("core_chat_hooks"), "read again").toHaveLength(2);
    });

    it("keeps no language for None, and no switch for a hook the tool does not bind to", async () => {
      installed.value = [{ ...TRANSLATOR, hooks: { message: TRANSLATOR.hooks!.message, settings: TRANSLATOR.hooks!.settings } }];
      const frame = runner(() => ({ values: { lang: "", receive: true, send: true }, auto: { message: true, composer: true }, lang: "" }));
      const { hooks } = conversation({ frame });
      await flushPromises();
      await hooks.settings(installed.value[0], { manual: true, values: { lang: "" } });
      expect(invoked("core_chat_hooks_set")).toEqual([{ conversation: "ft_bob", plugin: TRANSLATOR.id, autoMessage: true, autoComposer: false, lang: null }]);
    });

    it("applies an answer only after a yes to the tool's question", async () => {
      const frame = runner((hook) => (hook.answer ? ANSWER : { ask: { text: "Download French?" } }));
      const yes = conversation({ frame });
      await flushPromises();
      expect(await yes.hooks.settings(TRANSLATOR, { manual: true, values: VALUES })).toEqual(ANSWER);
      expect(frame.handed.map((hook) => hook.answer)).toEqual([undefined, true]);
      expect(invoked("core_chat_hooks_set")).toHaveLength(1);
      yes.scope.stop();

      alert.role = "cancel";
      const no = conversation({ frame: runner(() => ({ ask: { text: "Download French?" } })) });
      await flushPromises();
      expect(await no.hooks.settings(TRANSLATOR, { manual: true, values: VALUES })).toBeNull();
      expect(invoked("core_chat_hooks_set"), "nothing more kept").toHaveLength(1);
    });

    it("reads a tool's settings out of sight for its group, with the latest texts received", async () => {
      const frame = runner(() => ANSWER);
      const { hooks } = conversation({ frame, active: false, sample: "salut" });
      await flushPromises();
      expect(await hooks.readSettings(TRANSLATOR)).toEqual(ANSWER);
      expect(frame.handed).toEqual([{ name: "settings", text: "salut", chat: KEY, ref: null, lang: "en", manual: false }]);
    });

    it("finds the language of a conversation without one as it opens, from the latest texts received, once", async () => {
      const sample = "texte 2\ntexte 3\ntexte 4\ntexte 5\ntexte 6";
      const frame = runner(() => ({ lang: "fr" }));
      const { hooks } = conversation({ messages: [text("r1", "texte 6")], frame, sample });
      await vi.waitFor(() => expect(invoked("core_chat_hooks_set")).toHaveLength(1));
      expect(invoked("core_hook_sample")).toEqual([{ plugin: TRANSLATOR.id, conversation: "ft_bob" }]);
      expect(frame.handed).toEqual([{ name: "settings", text: sample, chat: KEY, ref: null, lang: "en", manual: false }]);
      expect(invoked("core_chat_hooks_set")[0]).toEqual({ conversation: "ft_bob", plugin: TRANSLATOR.id, autoMessage: false, autoComposer: false, lang: "fr" });
      await hooks.refresh();
      await flushPromises();
      expect(frame.handed, "once").toHaveLength(1);
    });

    it("does not look for a language a conversation has, nor without texts received or while unseen", async () => {
      const frame = runner(() => ({ lang: "fr" }));
      for (const setup of [
        { messages: [text("r1", "salut")], flags: [{ plugin: TRANSLATOR.id, autoMessage: false, autoComposer: false, lang: "es" }], sample: "salut" },
        { messages: [text("mine", "hola", true)], sample: "" },
        { messages: [text("r9", "salut")], active: false, sample: "salut" },
      ]) {
        const one = conversation({ ...setup, frame });
        await flushPromises();
        await flushPromises();
        one.scope.stop();
      }
      expect(frame.handed).toEqual([]);
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
