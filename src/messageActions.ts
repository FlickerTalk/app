import { computed, reactive, ref, type Ref } from "vue";
import { deleteForEveryone, forgetMessage, forwardMessage, loadMessages, pinMessage, react, shareMessage, store, type ChatMessage } from "./core";

export function useMessageActions(options: {
  chatId: () => string;
  messages: () => ChatMessage[];
  draft: Ref<string>;
  editing: Ref<ChatMessage | null>;
  replying: Ref<ChatMessage | null>;
}) {
  const acting = ref("");
  const folded = reactive(new Set<string>());
  const forwarding = ref(false);
  const erasing = ref(false);
  const openingWith = ref(false);

  const others = computed(() => store.chats.filter((one) => one.id !== options.chatId()));
  const actingMessage = computed(() => options.messages().find((message) => message.id === acting.value));
  const canEdit = computed(() => {
    const message = actingMessage.value;
    return message?.mine === true && message.kind !== "file" && !message.deleted && message.status !== "unsent";
  });

  function act(id: string) {
    acting.value = id;
    forwarding.value = false;
    erasing.value = false;
    openingWith.value = false;
  }

  function closeActions() {
    acting.value = "";
    forwarding.value = false;
    erasing.value = false;
    openingWith.value = false;
  }

  function fold() {
    const id = acting.value;
    if (folded.has(id)) folded.delete(id);
    else folded.add(id);
    closeActions();
  }

  async function share() {
    const id = acting.value;
    closeActions();
    await shareMessage(id).catch(() => {});
  }

  async function erase() {
    const id = acting.value;
    closeActions();
    folded.delete(id);
    await forgetMessage(id).catch(() => {});
    await loadMessages(options.chatId());
  }

  async function forwardTo(contact: string) {
    const id = acting.value;
    closeActions();
    await forwardMessage(id, contact).catch(() => {});
  }

  async function reactWith(emoji: string) {
    const id = acting.value;
    const same = actingMessage.value?.reactions?.mine === emoji;
    closeActions();
    await react(options.chatId(), id, same ? null : emoji).catch(() => {});
  }

  async function togglePin() {
    const id = acting.value;
    const was = actingMessage.value?.pinned === true;
    closeActions();
    await pinMessage(id, !was).catch(() => {});
  }

  function reply() {
    const id = acting.value;
    closeActions();
    options.replying.value = options.messages().find((message) => message.id === id) ?? null;
  }

  function edit() {
    const message = actingMessage.value;
    closeActions();
    if (!message) return;
    options.replying.value = null;
    options.editing.value = message;
    options.draft.value = message.text;
  }

  async function eraseEverywhere() {
    const id = acting.value;
    closeActions();
    folded.delete(id);
    await deleteForEveryone(id).catch(() => {});
  }

  return {
    acting,
    folded,
    forwarding,
    erasing,
    openingWith,
    others,
    actingMessage,
    canEdit,
    act,
    closeActions,
    fold,
    share,
    erase,
    forwardTo,
    reactWith,
    togglePin,
    reply,
    edit,
    eraseEverywhere,
  };
}
