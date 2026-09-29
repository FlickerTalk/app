/**
 * The on-screen keyboard (2026-09-29). Neither WebView shrinks the page when the keyboard opens:
 * Android's (edge-to-edge) and iOS's WKWebView only shrink the *visual* viewport and pan it to show
 * the focused field, so a page as tall as the window lost its header above the screen. The app
 * follows the visual viewport instead: `ion-app` is as tall as what can be seen and starts where
 * it starts (`--ft-viewport-height`, `--ft-viewport-top`, used in `theme/base.css`). With the field
 * already in sight the WebView has nothing left to pan, the header stays at the top, the content
 * shrinks and the footer sits on the keyboard.
 */
import { onMounted, onUnmounted, type Ref } from "vue";

/** Less than this is a folding bar or iOS's shortcut bar, not a keyboard (Ionic uses 150 too). */
const KEYBOARD_MIN = 150;
/** Pixels from the end that still count as «at the end» of a conversation. */
const END_SLACK = 32;

export interface ViewportSample {
  /** `window.innerHeight`: the page, which the keyboard does not shrink. */
  layoutHeight: number;
  /** `visualViewport.height` and `offsetTop`: what can be seen of it. */
  height: number;
  offsetTop: number;
}

export interface ViewportFit {
  height: number;
  top: number;
  /** How much of the page the keyboard covers. */
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

/** Whether a scroller shows its last pixels (or everything), to stay there when it shrinks. */
export function isAtEnd({ scrollHeight, clientHeight, scrollTop }: ScrollMetrics): boolean {
  return scrollHeight - clientHeight - scrollTop <= END_SLACK;
}

export interface ViewportWatcher {
  /** Right before the app changes size: the layout is still the old one. */
  before(): void;
  /** Right after, with the new size applied. */
  after(): void;
}

const watchers = new Set<ViewportWatcher>();

/** Hears each change of the app's height (not of its position alone). Returns the way out. */
export function watchViewport(watcher: ViewportWatcher): () => void {
  watchers.add(watcher);
  return () => watchers.delete(watcher);
}

/**
 * A conversation that showed its last message keeps showing it when the app shrinks for the
 * keyboard (or grows back); one scrolled back in time stays where it was. `scroller` gives the
 * scrolling element, or null while there is none yet. Returns the way out.
 */
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

/** What a conversation keeps of its `ion-content`. */
export type Scrollable = {
  $el?: { scrollToBottom?: (duration: number) => Promise<void>; getScrollElement?: () => Promise<HTMLElement> };
};

/** `stickToEnd` for a component's `ion-content`, for as long as the component lives. */
export function useStickToEnd(content: Ref<Scrollable | null>): void {
  let scroller: ScrollMetrics | null = null;
  const unstick = stickToEnd(() => scroller);
  onMounted(async () => {
    scroller = (await content.value?.$el?.getScrollElement?.()) ?? null;
  });
  onUnmounted(unstick);
}

/**
 * Keeps `root` (the document element) sized to the visual viewport of `win`, and tells Ionic when
 * the keyboard comes and goes with the events Capacitor's Keyboard plugin sends, which
 * `ion-footer` and `ion-tab-bar` listen to (the tab bar hides above the keyboard). Returns the way
 * to stop.
 */
export function startViewportFit(win: Window, root: HTMLElement): () => void {
  const viewport = win.visualViewport;
  if (!viewport) return () => {};
  let last: ViewportFit | null = null;

  const apply = () => {
    const fit = fitViewport({ layoutHeight: win.innerHeight, height: viewport.height, offsetTop: viewport.offsetTop });
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
