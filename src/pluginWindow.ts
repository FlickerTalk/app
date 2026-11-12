import { computed, onUnmounted, ref, watch, type Ref } from "vue";
import { useRouter } from "vue-router";
import { grantPlugin, markRead, type ChatMessage, type PickedFile, type PluginView, type Sending } from "./core";
import { installed, refreshPlugins, type HandedFile } from "./plugins";
import { closeOnBack } from "./back";
import { missingPermission, withPermission, type PermissionNeed } from "./permissions";
import { watchViewport } from "./viewport";

export type Opening = {
  id: string;
  name: string;
  sending: Sending;
  live: boolean;
  text?: string;
  file?: HandedFile;
  reference?: string;
  game?: boolean;
};
export type OpenPlugin = Opening & { contact: string; opening: number };

export function usePluginWindow(options: {
  chatId: () => string;
  active: () => boolean;
  split: () => boolean;
  messages: () => ChatMessage[];
  sheet: () => { close: () => unknown } | null | undefined;
  gameArea: () => HTMLElement | null | undefined;
  scrollToEnd: () => Promise<void>;
  draft: Ref<string>;
  staged: Ref<PickedFile | null>;
}) {
  const router = useRouter();
  const plugin = ref<OpenPlugin | null>(null);
  // every way out goes through the sheet's close(), so the other side hears the plugin left
  const leaving = ref(false);
  let next: OpenPlugin | null = null;
  let openings = 0;

  function openPlugin(chosen: Opening) {
    const opening = { ...chosen, contact: options.chatId(), opening: (openings += 1) };
    if (!plugin.value) {
      plugin.value = opening;
      return;
    }
    next = opening;
    closePlugin();
  }

  function closePlugin() {
    if (!plugin.value || leaving.value) return;
    answerNeed(false);
    leaving.value = true;
    const sheet = options.sheet();
    if (sheet) void sheet.close();
    else pluginClosed();
  }

  function pluginClosed() {
    leaving.value = false;
    plugin.value = next;
    next = null;
  }

  // Back stays taken while the plugin says goodbye, or a second press leaves the chat
  const holding = () => options.active() && leaving.value;
  let releaseHold: (() => void) | undefined;
  function holdBack() {
    releaseHold = holding() ? closeOnBack(holdBack) : undefined;
  }
  watch(holding, (now) => {
    releaseHold?.();
    releaseHold = undefined;
    if (now) holdBack();
  });
  onUnmounted(() => releaseHold?.());

  const playing = computed(() => Boolean(plugin.value?.game) && !leaving.value);
  const peeking = ref(false);
  const seenUpTo = ref(0);
  const lastFromThem = computed(() =>
    options
      .messages()
      .slice(seenUpTo.value)
      .filter((one) => !one.mine)
      .at(-1),
  );
  watch(playing, async (now) => {
    peeking.value = false;
    seenUpTo.value = options.messages().length;
    if (now) return;
    void markRead(options.chatId());
    await options.scrollToEnd();
  });
  async function peek() {
    peeking.value = !peeking.value;
    seenUpTo.value = options.messages().length;
    if (peeking.value) {
      await markRead(options.chatId());
      await options.scrollToEnd();
    }
  }
  // with the keyboard gone, show the game from its top again
  let keyboardWasOpen = false;
  const unwatchKeyboard = watchViewport({
    before: () => (keyboardWasOpen = document.documentElement.classList.contains("ft-keyboard-open")),
    after: () => {
      const gameArea = options.gameArea();
      if (keyboardWasOpen && !document.documentElement.classList.contains("ft-keyboard-open") && gameArea) gameArea.scrollTop = 0;
    },
  });
  onUnmounted(unwatchKeyboard);

  // the done that follows a staged file doesn't end the game
  let stagedByGame = false;
  function pluginDone() {
    if (plugin.value?.game && stagedByGame) {
      stagedByGame = false;
      return;
    }
    closePlugin();
  }

  // pushing the address already on screen does nothing, close the window instead
  function openChatFromPlugin(contact: string) {
    if (contact === options.chatId()) closePlugin();
    else void router.push(`/chat/${contact}`);
  }

  const needing = ref<PermissionNeed | null>(null);
  const openView = computed(() => installed.value.find((one) => one.id === plugin.value?.id));
  function answerNeed(allowed: boolean) {
    const need = needing.value;
    needing.value = null;
    need?.answer(allowed);
  }

  const askingFor = ref<PluginView | null>(null);
  const askingView = computed(() => askingFor.value ?? openView.value);

  function allowedToRead(chosen: PluginView): Promise<PluginView | undefined> {
    const line = missingPermission(chosen, "messages");
    if (!line) return Promise.resolve(undefined);
    return new Promise((resolve) => {
      let answered = false;
      askingFor.value = chosen;
      needing.value = {
        ...line,
        answer: async (allowed) => {
          if (answered) return;
          answered = true;
          if (!allowed) return resolve(undefined);
          try {
            await grantPlugin(chosen.id, withPermission(chosen, "messages", true));
            const now = (await refreshPlugins()).find((one) => one.id === chosen.id);
            resolve(now?.granted.messages ? now : undefined);
          } catch {
            resolve(undefined);
          }
        },
      };
    });
  }

  function needFromApp(need: PermissionNeed) {
    askingFor.value = null;
    needing.value = need;
  }

  // a plugin proposes, the user sends
  function fromPlugin(text: string) {
    options.draft.value = text;
    if (!plugin.value?.game) closePlugin();
  }

  function stage(file: PickedFile) {
    options.staged.value = file;
    if (plugin.value?.game) stagedByGame = true;
    else closePlugin();
  }

  // split view only, a chat page's chatId follows the address of a page pushed over it
  watch(options.chatId, () => {
    if (!options.split() || !options.active()) return;
    next = null;
    closePlugin();
  });

  return {
    plugin,
    leaving,
    openPlugin,
    closePlugin,
    pluginClosed,
    playing,
    peeking,
    lastFromThem,
    peek,
    pluginDone,
    openChatFromPlugin,
    needing,
    askingFor,
    askingView,
    answerNeed,
    allowedToRead,
    needFromApp,
    fromPlugin,
    stage,
  };
}
