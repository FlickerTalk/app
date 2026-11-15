import { computed, onBeforeUnmount, ref, watchEffect, type Ref } from "vue";
import { isChatListCollapsed, setChatListCollapsed } from "./preferences";

const ROOT_CLASS = "ft-list-collapsed";

// the root carries the mark because the chat pane's sheets open outside the page and size themselves on it
export function useChatListCollapse(wide: Readonly<Ref<boolean>>) {
  const chosen = ref(isChatListCollapsed());
  const collapsed = computed(() => wide.value && chosen.value);
  watchEffect(() => document.documentElement.classList.toggle(ROOT_CLASS, collapsed.value));
  onBeforeUnmount(() => document.documentElement.classList.remove(ROOT_CLASS));

  function toggle() {
    chosen.value = !chosen.value;
    setChatListCollapsed(chosen.value);
  }

  return { collapsed, toggle };
}
