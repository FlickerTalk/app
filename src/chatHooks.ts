/**
 * The chat's hooks in one conversation (2026-10-20, plan of the translator §8). A tool bound to
 * them runs in a frame of its own, out of sight (`PluginSheet` with `hidden`), one run at a time and
 * for HOOK_LIMIT at most; the app hands it a message, a draft or the conversation and shows what it
 * answers: a note under the message, a new draft, or the conversation's switches. With a switch on,
 * the tool runs by itself on each text received (only while the conversation is on screen) and on
 * each text sent. The app knows nothing of what the tool does: it never reads a language.
 */
import { computed, onScopeDispose, reactive, ref, watch } from "vue";
import { toastController } from "@ionic/vue";
import {
  chatHooks,
  pluginChat,
  pluginRef,
  setChatHooks,
  setMessageNote,
  type ChatHookState,
  type ChatMessage,
  type PluginHookName,
  type PluginView,
} from "./core";
import { i18n, t } from "./i18n";
import { installed, isLocked, pluginName, type PluginHook, type PluginHookResult } from "./plugins";

/** How long a hook may take, a download included (plan §8.1); then nothing is kept and the user is told. */
export const HOOK_LIMIT = 5 * 60_000;
/** How many of the latest texts received are run through the message hook as the conversation opens. */
export const CATCH_UP = 20;
/** How long the hooks' notices stay, in ms. */
const NOTICE = 3000;

/** A frame that runs hooks: the conversation's out of sight, or the tool's window for `chat`. */
export interface HookRunner {
  runHook(hook: PluginHook): Promise<PluginHookResult | null>;
}

/** A run that did not come back in time. */
const LATE = Symbol("late");

type HookAsked = Omit<PluginHook, "chat" | "lang">;

export function useChatHooks(options: { contact: () => string; messages: () => ChatMessage[]; active: () => boolean }) {
  /** The tools with a frame out of sight in this conversation, by id; the thread mounts one for each. */
  const frames = ref<string[]>([]);
  /** The frames on screen because their tool asked the user something (`ft.show`). */
  const shown = reactive(new Set<string>());
  /** The messages a hook the user started is working on, for the bubble to say so. */
  const noting = reactive(new Set<string>());
  /** Which tools run by themselves here, as the core says. */
  const flags = ref<ChatHookState[]>([]);

  const runners = new Map<string, HookRunner>();
  const waiting = new Map<string, (runner: HookRunner) => void>();
  const queues = new Map<string, Promise<unknown>>();
  /** What ran by itself already, by tool, message and words: each text once while the chat is open. */
  const tried = new Set<string>();
  /** Bumped when the conversation changes or goes: what was asked before comes back with nothing. */
  let generation = 0;

  const nobody: HookRunner = { runHook: async () => null };

  /** The thread hands over the frame it mounted for a tool (or `null` as it goes). */
  function attach(id: string, runner: HookRunner | null) {
    if (!runner) {
      runners.delete(id);
      return;
    }
    runners.set(id, runner);
    waiting.get(id)?.(runner);
    waiting.delete(id);
  }

  function setShown(id: string, visible: boolean) {
    if (visible) shown.add(id);
    else shown.delete(id);
  }

  /** The tool's frame out of sight, mounted first if it is not there. */
  function frameFor(id: string): Promise<HookRunner> {
    const ready = runners.get(id);
    if (ready && frames.value.includes(id)) return Promise.resolve(ready);
    if (!frames.value.includes(id)) frames.value = [...frames.value, id];
    return new Promise((resolve) => waiting.set(id, resolve));
  }

  /** Takes a tool's frame away: one that did not answer in time is not trusted with the next run. */
  function drop(id: string) {
    frames.value = frames.value.filter((one) => one !== id);
    runners.delete(id);
    shown.delete(id);
    waiting.get(id)?.(nobody);
    waiting.delete(id);
  }

  function forgetAll() {
    generation += 1;
    for (const resolve of waiting.values()) resolve(nobody);
    waiting.clear();
    runners.clear();
    queues.clear();
    shown.clear();
    tried.clear();
    frames.value = [];
  }

  async function notice(message: string) {
    try {
      const toast = await toastController.create({ message, duration: NOTICE, position: "top" });
      await toast.present();
    } catch {
      // No toast is no reason to stop.
    }
  }

  /**
   * Runs one hook in the tool's frame out of sight, after the runs asked before it. `ask` builds
   * the hook when its turn comes, or says there is nothing to run any more (`null`).
   */
  function run(plugin: PluginView, ask: () => Promise<HookAsked | null>): Promise<PluginHookResult | null | typeof LATE> {
    const at = generation;
    const before = queues.get(plugin.id) ?? Promise.resolve();
    const now = before.then(() => (at === generation ? runOnce(plugin, ask, at) : null));
    queues.set(plugin.id, now.catch(() => null));
    return now;
  }

  async function runOnce(plugin: PluginView, ask: () => Promise<HookAsked | null>, at: number): Promise<PluginHookResult | null | typeof LATE> {
    const hook = await ask();
    if (!hook || at !== generation) return null;
    let chat: string;
    try {
      chat = await pluginChat(plugin.id, options.contact());
    } catch {
      return null;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<typeof LATE>((resolve) => (timer = setTimeout(() => resolve(LATE), HOOK_LIMIT)));
    const answered = frameFor(plugin.id).then((runner) => runner.runHook({ ...hook, chat, lang: i18n.global.locale.value }));
    const result = await Promise.race([answered, late]).catch(() => null);
    clearTimeout(timer);
    if (at !== generation) return null;
    if (result === LATE) drop(plugin.id);
    return result;
  }

  /** A tool bound to `name` that may run now by itself: allowed to read what it is handed, not locked. */
  function usable(id: string, name: PluginHookName): PluginView | undefined {
    const view = installed.value.find((one) => one.id === id);
    return view && view.kind !== "game" && view.hooks?.[name] && view.granted.messages && !isLocked(view) ? view : undefined;
  }

  /**
   * The message hook on one text: what the tool answers is kept as a note under it. A run the user
   * started (`manual`) shows on the bubble meanwhile; one the conversation started runs only while
   * its switch is on and the chat is seen. Whether a note was kept.
   */
  async function noteMessage(plugin: PluginView, message: ChatMessage, manual = true): Promise<boolean> {
    if (manual) noting.add(message.id);
    try {
      const result = await run(plugin, async () => {
        if (!manual && !(options.active() && autoMessage.value.some((one) => one.id === plugin.id))) return null;
        const reference = await pluginRef(plugin.id, message.id).catch(() => null);
        return { name: "message", text: message.text, ref: reference, manual };
      });
      if (result === LATE) {
        void notice(t("chat.hookFailed", { plugin: pluginName(plugin) }));
        return false;
      }
      const said = result?.text?.trim();
      if (!said) return false;
      await setMessageNote(message.id, plugin.id, said);
      return true;
    } catch {
      return false;
    } finally {
      if (manual) noting.delete(message.id);
    }
  }

  /** The composer hook on the draft, started by the user: the new draft, or nothing. */
  async function compose(plugin: PluginView, draft: string): Promise<string | null> {
    const result = await run(plugin, async () => ({ name: "composer", text: draft, ref: null, manual: true }));
    if (result === LATE) {
      void notice(t("chat.hookFailed", { plugin: pluginName(plugin) }));
      return null;
    }
    return result?.text?.trim() || null;
  }

  /** The tool that translates what is sent here by itself, if its switch is on and it may run. */
  const autoComposer = computed(() =>
    flags.value.filter((flag) => flag.autoComposer).map((flag) => usable(flag.plugin, "composer")).find(Boolean),
  );
  const autoMessage = computed(() =>
    flags.value.filter((flag) => flag.autoMessage).map((flag) => usable(flag.plugin, "message")).filter((one): one is PluginView => Boolean(one)),
  );

  /**
   * What goes out for a text the user sends: with the conversation's composer switch on, what the
   * tool answers; unchanged, the same text; nothing (or no answer in time), the text as written,
   * and the user is told.
   */
  async function outgoing(text: string): Promise<string> {
    const plugin = autoComposer.value;
    if (!plugin) return text;
    const result = await run(plugin, async () => ({ name: "composer", text, ref: null, manual: false }));
    const said = result === LATE ? "" : result?.text?.trim();
    if (said) return said;
    void notice(t("chat.sentAsIs", { plugin: pluginName(plugin) }));
    return text;
  }

  /**
   * The chat hook, in the tool's window (`runner`): its switches are kept for the conversation, only
   * for the hooks it binds to. Closed before it answers, nothing is kept.
   */
  async function chooseForChat(plugin: PluginView, runner: HookRunner): Promise<void> {
    const contact = options.contact();
    let chat: string;
    try {
      chat = await pluginChat(plugin.id, contact);
    } catch {
      return;
    }
    const result = await runner.runHook({ name: "chat", text: "", chat, ref: null, lang: i18n.global.locale.value, manual: true }).catch(() => null);
    if (!result?.auto || options.contact() !== contact) return;
    const auto = { message: Boolean(result.auto.message && plugin.hooks?.message), composer: Boolean(result.auto.composer && plugin.hooks?.composer) };
    try {
      await setChatHooks(contact, plugin.id, auto);
    } catch {
      return;
    }
    await refresh();
  }

  /** Whether a tool runs by itself here, either way: its header button says so. */
  function isOn(id: string): boolean {
    return flags.value.some((flag) => flag.plugin === id && (flag.autoMessage || flag.autoComposer));
  }

  async function refresh() {
    const at = generation;
    const now = await chatHooks(options.contact());
    if (at === generation) flags.value = now;
  }

  /**
   * With the message switch on: the latest texts received without the tool's note, the newest first
   * (the ones in sight), as the chat opens and as they arrive. Only while the chat is seen.
   */
  function catchUp() {
    if (!options.active()) return;
    for (const plugin of autoMessage.value) {
      const waitingForIt = options
        .messages()
        .filter((one) => !one.mine && one.kind !== "file" && !one.deleted && one.text.trim() && !one.notes?.some((note) => note.plugin === plugin.id));
      for (const message of waitingForIt.slice(-CATCH_UP).reverse()) {
        const key = `${plugin.id}\n${message.id}\n${message.text}`;
        if (tried.has(key)) continue;
        tried.add(key);
        void noteMessage(plugin, message, false);
      }
    }
  }

  watch(
    options.contact,
    () => {
      forgetAll();
      flags.value = [];
      void refresh();
    },
    { immediate: true },
  );
  watch(() => [options.messages(), autoMessage.value, options.active()], catchUp);
  onScopeDispose(forgetAll);

  return { frames, shown, noting, attach, setShown, noteMessage, compose, outgoing, chooseForChat, isOn, refresh };
}
