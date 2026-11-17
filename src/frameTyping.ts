import { nextTick, onMounted, onUnmounted, ref, type Ref } from "vue";
import { fieldFromFrame } from "./frame-messages";
import { watchViewport } from "./viewport";

const FIELD_MARGIN = 8; // px

type Field = { top: number; bottom: number };

// the app can't see into a plugin's frame, but a field focused in there makes the frame this document's active element
export function useTypingInFrame(frame: () => HTMLIFrameElement | null | undefined, room: () => HTMLElement | null | undefined): Ref<boolean> {
  const typing = ref(false);
  let field: Field | null = null;
  const reveal = () => {
    const [shown, scroller] = [frame(), room()];
    if (!typing.value || !field || !shown || !scroller) return;
    const at = shown.getBoundingClientRect().top;
    const { top, bottom } = scroller.getBoundingClientRect();
    if (at + field.bottom > bottom) scroller.scrollTop += at + field.bottom - bottom + FIELD_MARGIN;
    else if (at + field.top < top) scroller.scrollTop -= top - at - field.top + FIELD_MARGIN;
  };
  const check = () => {
    const shown = frame();
    const was = typing.value;
    typing.value = Boolean(shown) && document.activeElement === shown && document.documentElement.classList.contains("ft-keyboard-open");
    if (typing.value && !was) void nextTick(reveal);
  };
  const heard = (event: MessageEvent) => {
    const said = fieldFromFrame(event, frame() ?? null);
    if (!said) return;
    field = said;
    reveal();
  };
  // the focus reaches the frame only after this window has heard it lose it
  const soon = () => setTimeout(check);
  let unwatch = () => {};
  onMounted(() => {
    unwatch = watchViewport({ before: () => {}, after: check });
    document.addEventListener("focusin", check);
    window.addEventListener("blur", soon);
    window.addEventListener("focus", soon);
    window.addEventListener("message", heard);
  });
  onUnmounted(() => {
    unwatch();
    document.removeEventListener("focusin", check);
    window.removeEventListener("blur", soon);
    window.removeEventListener("focus", soon);
    window.removeEventListener("message", heard);
  });
  return typing;
}
