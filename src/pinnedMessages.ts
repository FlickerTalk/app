import { computed, ref, watch } from "vue";
import type { ChatMessage } from "./core";
import { jumpTo } from "./chatSearch";

export function usePinnedMessages(messages: () => ChatMessage[]) {
  const pinned = computed(() => messages().filter((message) => message.pinned).reverse());
  const pinnedAt = ref(0);
  const shownPin = computed(() => pinned.value[pinnedAt.value % Math.max(pinned.value.length, 1)]);
  watch(pinned, (now) => (pinnedAt.value = now.length ? pinnedAt.value % now.length : 0));

  function visitPin() {
    const target = shownPin.value;
    if (!target) return;
    jumpTo(target.id);
    pinnedAt.value = (pinnedAt.value + 1) % pinned.value.length;
  }

  const pinText = (message: ChatMessage) => (message.kind === "file" ? message.file?.name ?? "" : message.text);

  return { pinned, pinnedAt, shownPin, visitPin, pinText };
}
