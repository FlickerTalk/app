import { ref, watch } from "vue";
import { pickFiles, sendFile, sendPicked, sendTyping, takePhoto, type ChatMessage, type PickedFile } from "./core";
import { t } from "./i18n";

export function useComposer(options: { chatId: () => string; isRequest: () => boolean; picker: () => HTMLInputElement | null | undefined }) {
  const draft = ref("");

  const editing = ref<ChatMessage | null>(null);
  watch(options.chatId, () => (editing.value = null));

  function cancelEdit() {
    editing.value = null;
    draft.value = "";
  }

  const replying = ref<ChatMessage | null>(null);
  watch(options.chatId, () => (replying.value = null));

  // no typing signal to a stranger still in the requests
  watch(draft, (now, before) => {
    if (now.trim() && now !== before && !options.isRequest()) void sendTyping(options.chatId());
  });

  const emoji = ref(false);

  function addEmoji(one: string) {
    draft.value += one;
  }

  async function attach(event: Event) {
    const input = event.target as HTMLInputElement;
    const files = Array.from(input.files ?? []);
    input.value = "";
    for (const file of files) {
      await sendFile(options.chatId(), file);
    }
  }

  // never the WebView's file input, it takes the user out of the app with no way back
  const attaching = ref(false);

  async function pick(accept: string) {
    attaching.value = false;
    try {
      for (const file of await pickFiles(accept)) {
        await sendPicked(options.chatId(), file);
      }
    } catch {
      options.picker()?.click();
    }
  }

  const composerError = ref("");

  async function snap() {
    attaching.value = false;
    composerError.value = "";
    let taken: Awaited<ReturnType<typeof takePhoto>>;
    try {
      taken = await takePhoto();
    } catch {
      composerError.value = t("chat.cannotTakePhoto");
      return;
    }
    for (const file of taken) {
      await sendPicked(options.chatId(), file);
    }
  }

  const staged = ref<PickedFile | null>(null);

  async function sendStaged() {
    const file = staged.value;
    if (!file) return;
    staged.value = null;
    await sendPicked(options.chatId(), file);
  }

  return { draft, editing, replying, emoji, attaching, staged, composerError, cancelEdit, addEmoji, attach, pick, snap, sendStaged };
}
