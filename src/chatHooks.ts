/**
 * The chat's hooks in one conversation (2026-10-20, plan of the translator §8, UX of §8.2d). A tool
 * bound to them runs in a frame of its own, out of sight (`PluginSheet` with `hidden`), one run at a
 * time and for HOOK_LIMIT at most. The app hands it a message or what the user sends, and shows what
 * it answers in the message's place. With the conversation's switches on, the tool runs by itself
 * on each text received (only while the conversation is on screen) and on each text sent, whose
 * original words are kept under it. The app knows nothing of what the tool does: it only compares
 * the conversation's language, as the tool found it, with its own, to offer the link or not.
 *
 * A conversation is a contact's or a circle's (by its id): circles reuse all of it.
 */
import { computed, onScopeDispose, reactive, ref, watch } from "vue";
import { alertController, toastController } from "@ionic/vue";
import {
  chatHooks,
  hookSample,
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

/** A frame that runs hooks out of sight (`PluginSheet` with `hidden`). */
export interface HookRunner {
  runHook(hook: PluginHook): Promise<PluginHookResult | null>;
}

/** A run that did not come back in time. */
const LATE = Symbol("late");
/** A run that failed: the frame threw, or the core would not say the chat's key. */
const FAILED = Symbol("failed");

type HookAsked = Omit<PluginHook, "chat" | "lang">;
type Ran = PluginHookResult | null | typeof LATE | typeof FAILED;

/** The settings' values by field key, as the settings hook hands them and is handed them. */
export type SettingsValues = Record<string, string | boolean>;

/** A text's own words, kept under it once a tool sent it in other words. */
export interface Original {
  plugin: string;
  text: string;
}

/** The main subtag of a language tag, to compare "en-GB" with "en". */
const primary = (tag: string) => tag.split(/[-_]/)[0].toLowerCase();

/** The phone's name for a language in the app's language ("fr" → "French", "francés"); "" if none. */
function phoneName(tag: string): string {
  try {
    const name = new Intl.DisplayNames([i18n.global.locale.value], { type: "language" }).of(tag) ?? "";
    // Android's WebView names no language for be, cy, eo, ga, ht, mt (2026-10-21): it says the tag.
    return name.toLowerCase() === tag.toLowerCase() ? "" : name;
  } catch {
    return "";
  }
}

/**
 * A language's name in the app's language: the phone's own; else the tool's, from the choices its
 * settings hook gave (`named`, not preferred: it says "(detected)" beside a language it found);
 * else the tag.
 */
export function languageName(tag: string | undefined, named: Record<string, string> = {}): string {
  if (!tag) return "";
  return phoneName(tag) || named[tag] || tag;
}

/** The tools' names for languages, by conversation, from the choices their settings hook gave. */
const toolNames = new Map<string, Record<string, string>>();

/** The conversations' switches as last read, so a conversation opened again shows them at once. */
const lastRead = new Map<string, ChatHookState[]>();

/**
 * Asks the user the tool's question (a download, §8.2c) in the app's own alert: whether they said
 * yes. Never fails: no alert is a no.
 */
export async function confirmAsk(text: string): Promise<boolean> {
  try {
    const alert = await alertController.create({
      message: text,
      buttons: [
        { text: t("common.cancel"), role: "cancel" },
        { text: t("common.ok"), role: "confirm" },
      ],
    });
    await alert.present();
    const { role } = await alert.onDidDismiss();
    return role === "confirm";
  } catch {
    return false;
  }
}

export async function notice(message: string) {
  try {
    const toast = await toastController.create({ message, duration: NOTICE, position: "top" });
    await toast.present();
  } catch {
    // No toast is no reason to stop.
  }
}

export function useChatHooks(options: { conversation: () => string; messages: () => ChatMessage[]; active: () => boolean }) {
  /** The tools with a frame out of sight in this conversation, by id; the host mounts one for each. */
  const frames = ref<string[]>([]);
  /** The frames on screen because their tool asked the user something (`ft.show`). */
  const shown = reactive(new Set<string>());
  /** The messages a hook the user started is working on, and the tool, for the bubble to say so. */
  const noting = reactive(new Map<string, string>());
  /** Which tools run by themselves here, and the conversation's language, as the core says. */
  const flags = ref<ChatHookState[]>(lastRead.get(options.conversation()) ?? []);

  const runners = new Map<string, HookRunner>();
  const waiting = new Map<string, (runner: HookRunner) => void>();
  const queues = new Map<string, Promise<unknown>>();
  /** What ran by itself already, by tool, message and words: each text once while the chat is open. */
  const tried = new Set<string>();
  /** Bumped when the conversation changes or goes: what was asked before comes back with nothing. */
  let generation = 0;
  /** The conversation's switches have been read from the core since it opened. */
  const loaded = ref(false);
  /** Its language was looked for since it opened: once, whatever the tool found. */
  let looked = false;

  const nobody: HookRunner = { runHook: async () => null };

  /** The host hands over the frame it mounted for a tool (or `null` as it goes). */
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
    noting.clear();
    frames.value = [];
    loaded.value = false;
    looked = false;
    askedName = false;
    named.value = toolNames.get(options.conversation()) ?? {};
  }

  /**
   * Runs one hook in the tool's frame out of sight, after the runs asked before it. `ask` builds
   * the hook when its turn comes, or says there is nothing to run any more (`null`).
   */
  function run(plugin: PluginView, ask: () => Promise<HookAsked | null>): Promise<Ran> {
    const at = generation;
    const before = queues.get(plugin.id) ?? Promise.resolve();
    const now = before.then(() => (at === generation ? runOnce(plugin, ask, at) : null));
    queues.set(plugin.id, now.catch(() => null));
    return now;
  }

  async function runOnce(plugin: PluginView, ask: () => Promise<HookAsked | null>, at: number): Promise<Ran> {
    const hook = await ask();
    if (!hook || at !== generation) return null;
    let chat: string;
    try {
      chat = await pluginChat(plugin.id, options.conversation());
    } catch {
      return FAILED;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const late = new Promise<typeof LATE>((resolve) => (timer = setTimeout(() => resolve(LATE), HOOK_LIMIT)));
    const answered = frameFor(plugin.id).then((runner) => runner.runHook({ ...hook, chat, lang: i18n.global.locale.value }));
    const result: Ran = await Promise.race([answered, late]).catch(() => FAILED);
    clearTimeout(timer);
    if (at !== generation) return null;
    if (result === LATE) drop(plugin.id);
    return result;
  }

  /**
   * One hook the user started, or a run of the conversation's own: a question in the answer
   * (§8.2c) is asked in the app's alert and, on a yes, the hook runs again with the answer; a run
   * of its own never asks. A tool that fails or is silent too long is named in a notice. The answer,
   * or nothing.
   */
  async function runAsking(plugin: PluginView, manual: boolean, ask: (answer: boolean) => Promise<HookAsked | null>): Promise<PluginHookResult | null> {
    let result = await run(plugin, () => ask(false));
    if (result !== LATE && result !== FAILED && result?.ask) {
      if (!manual || !(await confirmAsk(result.ask.text))) return null;
      result = await run(plugin, () => ask(true));
    }
    if (result === LATE || result === FAILED) {
      void notice(t("chat.hookFailed", { plugin: pluginName(plugin) }));
      return null;
    }
    return result?.ask ? null : result;
  }

  /** A tool bound to `name` that may run now: allowed to read what it is handed, not locked. */
  function usable(id: string, name: PluginHookName): PluginView | undefined {
    const view = installed.value.find((one) => one.id === id);
    return view && view.kind !== "game" && view.hooks?.[name] && view.granted.messages && !isLocked(view) ? view : undefined;
  }

  /**
   * The message hook on one text: what the tool answers is kept as a note, shown in the text's
   * place. A run the user started (`manual`) shows on the bubble meanwhile; one the conversation
   * started runs only while its switch is on and the chat is seen. Whether a note was kept.
   */
  async function noteMessage(plugin: PluginView, message: ChatMessage, manual = true): Promise<boolean> {
    if (manual) noting.set(message.id, plugin.id);
    try {
      const result = await runAsking(plugin, manual, async (answer) => {
        if (!manual && !(options.active() && autoMessage.value.some((one) => one.id === plugin.id))) return null;
        const reference = await pluginRef(plugin.id, message.id).catch(() => null);
        return { name: "message", text: message.text, ref: reference, manual, ...(answer ? { answer } : {}) };
      });
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

  /** The first tool of this phone bound to messages (never a game): its link goes under the texts. */
  const messageTool = computed(() => installed.value.find((one) => one.kind !== "game" && one.hooks?.message));

  /** The conversation's language as the tool found it or the user chose it; none when not known. */
  const language = computed(() => {
    const own = flags.value.find((flag) => flag.plugin === messageTool.value?.id)?.lang;
    return (own ?? flags.value.find((flag) => flag.lang)?.lang) || undefined;
  });
  /** The tool's names for languages here, as its settings hook last gave them. */
  const named = ref<Record<string, string>>(toolNames.get(options.conversation()) ?? {});
  const languageNameNow = computed(() => languageName(language.value, named.value));

  /** Whether received texts offer the message hook: a tool for it, and a language not the app's. */
  const offersHook = computed(() => Boolean(messageTool.value) && (!language.value || primary(language.value) !== primary(i18n.global.locale.value)));

  /** The tool that translates what is sent here by itself, if its switch is on and it may run. */
  const autoComposer = computed(() =>
    flags.value.filter((flag) => flag.autoComposer).map((flag) => usable(flag.plugin, "composer")).find(Boolean),
  );
  const autoMessage = computed(() =>
    flags.value.filter((flag) => flag.autoMessage).map((flag) => usable(flag.plugin, "message")).filter((one): one is PluginView => Boolean(one)),
  );

  /** The language the conversation is translated into by itself, by name; "" while nothing is. */
  const translating = computed(() => (language.value && (autoComposer.value || autoMessage.value.length) ? languageNameNow.value : ""));

  /**
   * What goes out for a text the user sends: with the conversation's composer switch on, what the
   * tool answers, and the user's own words to keep under it (`keepOriginal`); unchanged, the same
   * text; nothing (or no answer in time), the text as written, and the user is told.
   */
  async function outgoing(text: string): Promise<{ text: string; original?: Original }> {
    const plugin = autoComposer.value;
    if (!plugin) return { text };
    const result = await run(plugin, async () => ({ name: "composer", text, ref: null, manual: false }));
    const said = result === LATE || result === FAILED ? "" : result?.text?.trim();
    if (!said) {
      void notice(t("chat.sentAsIs", { plugin: pluginName(plugin) }));
      return { text };
    }
    return said === text ? { text } : { text: said, original: { plugin: plugin.id, text } };
  }

  /** The user's own words kept under the message that went in other words; none without its id. */
  async function keepOriginal(message: string | undefined, original: Original): Promise<void> {
    if (!message) return;
    await setMessageNote(message, original.plugin, original.text).catch(() => undefined);
  }

  async function refresh() {
    const at = generation;
    const conversation = options.conversation();
    const now = await chatHooks(conversation);
    lastRead.set(conversation, now);
    if (at !== generation) return;
    flags.value = now;
    loaded.value = true;
  }

  /**
   * The settings hook (§8.2d): run with the values the user changed (`manual`) or to read them
   * (`manual: false`, `text` the latest texts received, to find the language). A question in the
   * answer is asked first and, only after a yes, what it answers again is applied. The app keeps
   * the switches and the conversation's language it answers, for the hooks the tool binds to; a
   * switch or a language it does not say stays as it was. The answer (values and options for the
   * group), or nothing.
   */
  async function settings(plugin: PluginView, asked: { manual: boolean; values?: SettingsValues; text?: string }): Promise<PluginHookResult | null> {
    const conversation = options.conversation();
    const result = await runAsking(plugin, asked.manual, async (answer) => ({
      name: "settings",
      text: asked.text ?? "",
      ref: null,
      manual: asked.manual,
      ...(asked.values ? { values: asked.values } : {}),
      ...(answer ? { answer } : {}),
    }));
    if (result && options.conversation() === conversation) {
      learnNames(conversation, result);
      await keep(plugin, conversation, result);
    }
    return result;
  }

  function learnNames(conversation: string, result: PluginHookResult) {
    const choices = result.options?.lang?.filter((one) => one.value && one.label.trim());
    if (!choices?.length) return;
    const now = { ...(toolNames.get(conversation) ?? {}), ...Object.fromEntries(choices.map((one) => [one.value, one.label])) };
    toolNames.set(conversation, now);
    named.value = now;
  }

  /** Its language is one the phone cannot name, and the tool has not named it yet. */
  let askedName = false;
  async function lookForName() {
    const plugin = settingsTool.value;
    const tag = language.value;
    if (askedName || !tag || phoneName(tag) || named.value[tag] || !plugin || !options.active()) return;
    askedName = true;
    await readSettings(plugin);
  }

  async function keep(plugin: PluginView, conversation: string, result: PluginHookResult) {
    if (!result.auto && result.lang === undefined) return;
    const now = flags.value.find((flag) => flag.plugin === plugin.id);
    const said = {
      message: Boolean((result.auto?.message ?? now?.autoMessage) && plugin.hooks?.message),
      composer: Boolean((result.auto?.composer ?? now?.autoComposer) && plugin.hooks?.composer),
      lang: result.lang === undefined ? now?.lang : result.lang,
    };
    try {
      await setChatHooks(conversation, plugin.id, said);
    } catch {
      return;
    }
    await refresh();
  }

  /** A tool's settings as its group opens: read out of sight, with the latest texts received. */
  async function readSettings(plugin: PluginView): Promise<PluginHookResult | null> {
    return settings(plugin, { manual: false, text: await hookSample(plugin.id, options.conversation()) });
  }

  /** The first tool of this phone with a settings group (never a game), not locked. */
  const settingsTool = computed(() => installed.value.find((one) => one.kind !== "game" && one.hooks?.settings && !isLocked(one)));

  /**
   * A conversation without a language, once its switches are read and it is seen: the settings hook
   * runs out of sight once with the latest texts received (§8.2c), and what it finds is kept. Not
   * without texts to look at (the core gives them only to a tool allowed to read them).
   */
  async function lookForLanguage() {
    const plugin = settingsTool.value;
    if (looked || !loaded.value || language.value || !plugin || !options.active()) return;
    if (!options.messages().some((one) => !one.mine && one.kind !== "file" && !one.deleted && one.text.trim())) return;
    looked = true;
    const at = generation;
    const text = await hookSample(plugin.id, options.conversation());
    if (!text.trim() || at !== generation) return;
    await settings(plugin, { manual: false, text });
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
    options.conversation,
    (conversation) => {
      forgetAll();
      flags.value = lastRead.get(conversation) ?? [];
      void refresh();
    },
    { immediate: true },
  );
  // Back on screen (its switches may have changed on the contact's page meanwhile): read again.
  watch(options.active, (now, before) => {
    if (now && !before) void refresh();
  });
  watch(() => [options.messages(), autoMessage.value, options.active()], catchUp);
  watch(() => [loaded.value, language.value, settingsTool.value, options.messages(), options.active()], () => void lookForLanguage());
  watch(() => [language.value, settingsTool.value, options.active()], () => void lookForName());
  onScopeDispose(forgetAll);

  return {
    frames,
    shown,
    noting,
    flags,
    messageTool,
    language,
    languageName: languageNameNow,
    offersHook,
    translating,
    attach,
    setShown,
    dismiss: drop,
    noteMessage,
    settings,
    readSettings,
    outgoing,
    keepOriginal,
    refresh,
    catchUp,
  };
}

/** What `useChatHooks` hands its host: the frames to mount, and what the conversation shows. */
export type ChatHooks = ReturnType<typeof useChatHooks>;
