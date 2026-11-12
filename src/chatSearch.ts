import { nextTick, ref, watch } from "vue";
import { searchMessages, type ChatMessage } from "./core";

export type SearchField = HTMLElement & { setFocus?: () => Promise<void>; componentOnReady?: () => Promise<unknown> };

export function jumpTo(id: string) {
  const target = document.querySelector(`[data-message="${CSS.escape(id)}"]`);
  target?.scrollIntoView({ block: "center", behavior: "smooth" });
}

export function useChatSearch(options: {
  chatId: () => string;
  asked: () => boolean;
  active: () => boolean;
  field: () => SearchField | undefined;
}) {
  const searching = ref(false);
  const query = ref("");
  const hits = ref<ChatMessage[]>([]);
  const searched = ref(false);
  const lit = ref("");
  let searchTimer: ReturnType<typeof setTimeout> | undefined;

  function openSearch() {
    searching.value = true;
    query.value = "";
    hits.value = [];
    searched.value = false;
    void nextTick(focusSearch);
  }

  // opened as the page comes in, Ionic's field isn't ready yet, focus it once the page is on screen
  let focusLater = false;
  async function focusSearch() {
    const field = options.field();
    await field?.componentOnReady?.();
    await field?.setFocus?.();
    focusLater = searching.value && !field?.contains(document.activeElement);
  }
  watch(options.active, (on) => {
    if (on && focusLater) void focusSearch();
  });

  function typeSearch(value: unknown) {
    query.value = typeof value === "string" ? value : "";
  }

  function closeSearch() {
    searching.value = false;
    query.value = "";
    hits.value = [];
    searched.value = false;
  }

  watch(query, (now) => {
    clearTimeout(searchTimer);
    if (!now.trim()) {
      hits.value = [];
      searched.value = false;
      return;
    }
    searchTimer = setTimeout(async () => {
      const asked = now;
      const found = await searchMessages(options.chatId(), asked).catch(() => []);
      if (query.value === asked) {
        hits.value = found;
        searched.value = true;
      }
    }, 250);
  });
  watch(options.chatId, closeSearch);
  watch(
    options.asked,
    (asked) => {
      if (asked) openSearch();
    },
    { immediate: true },
  );

  function visitHit(id: string) {
    jumpTo(id);
    lit.value = id;
    setTimeout(() => (lit.value === id && (lit.value = "")), 1600);
  }

  return { searching, query, hits, searched, lit, openSearch, closeSearch, typeSearch, visitHit };
}
