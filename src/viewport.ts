// neither WebView shrinks the page for the keyboard, only the visual viewport, so a full-height page
// lost its header. The app follows the visual viewport instead.
import { onMounted, onUnmounted, type Ref } from "vue";

// below this it's a folding bar or iOS's shortcut bar, Ionic uses 150 too
const KEYBOARD_MIN = 150;
const END_SLACK = 32;

export interface ViewportSample {
  layoutHeight: number;
  height: number;
  offsetTop: number;
}

export interface ViewportFit {
  height: number;
  top: number;
  keyboard: number;
  open: boolean;
}

export function fitViewport({ layoutHeight, height, offsetTop }: ViewportSample): ViewportFit {
  const page = Math.round(layoutHeight);
  const visible = Math.min(Math.round(height), page);
  const top = Math.min(Math.max(Math.round(offsetTop), 0), page - visible);
  const keyboard = page - visible;
  return { height: visible, top, keyboard, open: keyboard >= KEYBOARD_MIN };
}

export interface ScrollMetrics {
  scrollHeight: number;
  clientHeight: number;
  scrollTop: number;
}

export function isAtEnd({ scrollHeight, clientHeight, scrollTop }: ScrollMetrics): boolean {
  return scrollHeight - clientHeight - scrollTop <= END_SLACK;
}

export interface ViewportWatcher {
  before(): void;
  after(): void;
}

const watchers = new Set<ViewportWatcher>();

export function watchViewport(watcher: ViewportWatcher): () => void {
  watchers.add(watcher);
  return () => watchers.delete(watcher);
}

export function stickToEnd(scroller: () => ScrollMetrics | null): () => void {
  let stuck = false;
  return watchViewport({
    before() {
      const element = scroller();
      stuck = element !== null && isAtEnd(element);
    },
    after() {
      const element = scroller();
      if (stuck && element) element.scrollTop = element.scrollHeight;
    },
  });
}

export function followEnd(scroller: HTMLElement, list: Element, Observer: typeof ResizeObserver = ResizeObserver): () => void {
  let atEnd = isAtEnd(scroller);
  const onScroll = () => (atEnd = isAtEnd(scroller));
  scroller.addEventListener("scroll", onScroll, { passive: true });
  const observer = new Observer(() => {
    if (atEnd) scroller.scrollTop = scroller.scrollHeight;
  });
  observer.observe(scroller);
  observer.observe(list);
  return () => {
    scroller.removeEventListener("scroll", onScroll);
    observer.disconnect();
  };
}

export type Scrollable = {
  $el?: { scrollToBottom?: (duration: number) => Promise<void>; getScrollElement?: () => Promise<HTMLElement> };
};

export function useStickToEnd(content: Ref<Scrollable | null>, list?: Ref<HTMLElement | null>): void {
  let scroller: ScrollMetrics | null = null;
  let unfollow: (() => void) | null = null;
  let gone = false;
  const unstick = stickToEnd(() => scroller);
  onMounted(async () => {
    scroller = (await content.value?.$el?.getScrollElement?.()) ?? null;
    if (!gone && scroller instanceof HTMLElement && list?.value && typeof ResizeObserver !== "undefined") unfollow = followEnd(scroller, list.value);
  });
  onUnmounted(() => {
    gone = true;
    unstick();
    unfollow?.();
  });
}

// ion-footer and ion-tab-bar listen for Capacitor's keyboard events, so send those
export function startViewportFit(win: Window, root: HTMLElement): () => void {
  const viewport = win.visualViewport;
  if (!viewport) return () => {};
  let last: ViewportFit | null = null;

  const apply = () => {
    // WKWebView shrinks innerHeight with the keyboard too
    const layoutHeight = root.clientHeight || win.innerHeight;
    const fit = fitViewport({ layoutHeight, height: viewport.height, offsetTop: viewport.offsetTop });
    const resized = last !== null && fit.height !== last.height;
    if (resized) for (const watcher of watchers) watcher.before();
    root.style.setProperty("--ft-viewport-height", `${fit.height}px`);
    root.style.setProperty("--ft-viewport-top", `${fit.top}px`);
    root.classList.toggle("ft-keyboard-open", fit.open);
    if (last !== null && fit.open !== last.open) {
      const event = fit.open
        ? new CustomEvent("keyboardWillShow", { detail: { keyboardHeight: fit.keyboard } })
        : new CustomEvent("keyboardWillHide");
      win.dispatchEvent(event);
    }
    last = fit;
    if (resized) for (const watcher of watchers) watcher.after();
  };

  apply();
  viewport.addEventListener("resize", apply);
  viewport.addEventListener("scroll", apply);
  return () => {
    viewport.removeEventListener("resize", apply);
    viewport.removeEventListener("scroll", apply);
  };
}
