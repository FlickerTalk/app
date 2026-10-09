// The call screen on the page (2026-10-09). The route leaves it at the start of its transition,
// but Ionic takes it away only at the end: a push back to the call in between found that leaving
// screen, and its removal left a blank page. Whoever goes back to the call waits for it to be gone.
// A call screen may also stay in the history under pages opened over it (a reminder's notification
// tapped during the call): going back to the call then goes back to it, pushing no second one.
import { reactive, watch } from "vue";
import type { Router } from "vue-router";
import { goBack } from "./back";
import { pageTransitionsDone } from "./page-transition";

type Screen = {
  /** The route it was mounted on. */
  path: string;
  /** Its place in the browser's history (vue-router's `history.state.position`), if known. */
  position: number | null;
  /** Still in the history: not gone back from, nor replaced. Once out, it is only leaving. */
  inHistory: boolean;
};

/** Call screens mounted: the one on screen, ones under pages opened over them, and ones leaving. */
const screens = reactive<Screen[]>([]);

/** Where the browser's history is now, as vue-router numbers its entries. */
function historyPosition(): number | null {
  const position = (window.history.state as { position?: unknown } | null)?.position;
  return typeof position === "number" ? position : null;
}

/**
 * A call screen is on the page; the function returned says it unmounted (once). With the router,
 * the screen follows its place in the history; without, it only counts as one that is leaving.
 */
export function callScreenMounted(router?: Router): () => void {
  const position = historyPosition();
  const screen: Screen = reactive({
    path: router?.currentRoute.value.path ?? "",
    position,
    inHistory: Boolean(router) && position !== null,
  });
  screens.push(screen);
  // Back to before it, or another page in its place: its entry is gone from the history.
  const stop = router?.afterEach((to) => {
    const here = historyPosition();
    if (screen.position === null || here === null) screen.inHistory = false;
    else if (here < screen.position || (here === screen.position && to.path !== screen.path)) screen.inHistory = false;
  });
  let done = false;
  return () => {
    if (done) return;
    done = true;
    stop?.();
    screens.splice(screens.indexOf(screen), 1);
  };
}

const leaving = () => screens.some((screen) => !screen.inHistory);

/** Resolves once no call screen is leaving the page: at once, or when the ones leaving have gone. */
export function callScreenGone(): Promise<void> {
  if (!leaving()) return Promise.resolve();
  return new Promise((resolve) => {
    const stop = watch(
      leaving,
      (still) => {
        if (still) return;
        stop();
        resolve();
      },
      { flush: "sync" },
    );
  });
}

/** The way to a call screen under way: another one waits for it, and finds itself there. */
let going: Promise<void> = Promise.resolve();

/**
 * To the call screen `path`: back in the history to the one under the pages on top if there is
 * one, otherwise a new one, once any call screen leaving the page has gone. Nothing on it already.
 */
export function showCallScreen(router: Router, path: string): Promise<void> {
  const next = going.then(() => toCallScreen(router, path));
  going = next.catch(() => undefined);
  return next;
}

async function toCallScreen(router: Router, path: string): Promise<void> {
  if (router.currentRoute.value.path === path) return;
  const here = historyPosition();
  const under =
    here === null
      ? undefined
      : screens.find((screen) => screen.inHistory && screen.path === path && screen.position !== null && screen.position < here);
  if (under?.position != null && here !== null) {
    // One page at a time: Ionic takes away the pages a jump of several steps goes over only in an
    // app with a single stack of pages, and this one has the tabs' too. And never while a page is
    // still moving: gone back to as the page on top came in, the call screen was left hidden.
    for (let steps = here - under.position; steps > 0; steps--) {
      await pageTransitionsDone();
      await goBack(router);
    }
    return;
  }
  await callScreenGone();
  await router.push(path);
}
