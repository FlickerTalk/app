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

// ms, a download included
export const HOOK_LIMIT = 5 * 60_000;
export const CATCH_UP = 20;
const NOTICE = 3000;

export interface HookRunner {
  runHook(hook: PluginHook): Promise<PluginHookResult | null>;
}

const LATE = Symbol("late");
const FAILED = Symbol("failed");

type HookAsked = Omit<PluginHook, "chat" | "lang">;
type Ran = PluginHookResult | null | typeof LATE | typeof FAILED;

export type SettingsValues = Record<string, string | boolean>;

export interface Original {
  plugin: string;
  text: string;
}

const primary = (tag: string) => tag.split(/[-_]/)[0].toLowerCase();

function phoneName(tag: string): string {
  try {
    const name = new Intl.DisplayNames([i18n.global.locale.value], { type: "language" }).of(tag) ?? "";
    // Android's WebView has no names for be, cy, eo, ga, ht, mt and returns the tag
    return name.toLowerCase() === tag.toLowerCase() ? "" : name;
  } catch {
    return "";
  }
}

// by grapheme and locale, so Turkish "i" becomes "İ"
export function capitalized(text: string, locale: string): string {
  const [first] = new Intl.Segmenter(locale, { granularity: "grapheme" }).segment(text);
  return first ? first.segment.toLocaleUpperCase(locale) + text.slice(first.segment.length) : text;
}

// the tool's names come last, they add "(detected)" to a language it found
export function languageName(tag: string | undefined, named: Record<string, string> = {}): string {
  if (!tag) return "";
  return phoneName(tag) || named[tag] || tag;
}

function aloneName(tag: string | undefined, named: Record<string, string>): string {
  const name = languageName(tag, named);
  return name && name !== tag ? capitalized(name, i18n.global.locale.value) : name;
}

const toolNames = new Map<string, Record<string, string>>();

const lastRead = new Map<string, ChatHookState[]>();

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
    // no toast is no reason to stop
  }
}

export function useChatHooks(options: { conversation: () => string; messages: () => ChatMessage[]; active: () => boolean }) {
  const frames = ref<string[]>([]);
  const shown = reactive(new Set<string>());
  const noting = reactive(new Map<string, string>());
  const flags = ref<ChatHookState[]>(lastRead.get(options.conversation()) ?? []);

  const runners = new Map<string, HookRunner>();
  const waiting = new Map<string, (runner: HookRunner) => void>();
  const queues = new Map<string, Promise<unknown>>();
  const tried = new Set<string>();
  // bumped when the conversation changes, so late answers are dropped
  let generation = 0;
  const loaded = ref(false);
  let looked = false;

  const nobody: HookRunner = { runHook: async () => null };

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

  function frameFor(id: string): Promise<HookRunner> {
    const ready = runners.get(id);
    if (ready && frames.value.includes(id)) return Promise.resolve(ready);
    if (!frames.value.includes(id)) frames.value = [...frames.value, id];
    return new Promise((resolve) => waiting.set(id, resolve));
  }

  // a frame that missed its deadline doesn't get the next run
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

  function usable(id: string, name: PluginHookName): PluginView | undefined {
    const view = installed.value.find((one) => one.id === id);
    return view && view.kind !== "game" && view.hooks?.[name] && view.granted.messages && !isLocked(view) ? view : undefined;
  }

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

  const messageTool = computed(() => installed.value.find((one) => one.kind !== "game" && one.hooks?.message));

  const language = computed(() => {
    const own = flags.value.find((flag) => flag.plugin === messageTool.value?.id)?.lang;
    return (own ?? flags.value.find((flag) => flag.lang)?.lang) || undefined;
  });
  const named = ref<Record<string, string>>(toolNames.get(options.conversation()) ?? {});
  const languageNameNow = computed(() => languageName(language.value, named.value));

  const offersHook = computed(() => Boolean(messageTool.value) && (!language.value || primary(language.value) !== primary(i18n.global.locale.value)));

  const autoComposer = computed(() =>
    flags.value.filter((flag) => flag.autoComposer).map((flag) => usable(flag.plugin, "composer")).find(Boolean),
  );
  const autoMessage = computed(() =>
    flags.value.filter((flag) => flag.autoMessage).map((flag) => usable(flag.plugin, "message")).filter((one): one is PluginView => Boolean(one)),
  );

  const translating = computed(() => (language.value && (autoComposer.value || autoMessage.value.length) ? aloneName(language.value, named.value) : ""));

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

  async function readSettings(plugin: PluginView): Promise<PluginHookResult | null> {
    return settings(plugin, { manual: false, text: await hookSample(plugin.id, options.conversation()) });
  }

  const settingsTool = computed(() => installed.value.find((one) => one.kind !== "game" && one.hooks?.settings && !isLocked(one)));

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
  // switches may have changed on the contact's page
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

export type ChatHooks = ReturnType<typeof useChatHooks>;
