// Ionic removes a leaving call screen only after its transition, and a push back to the call
// in between ended on a blank page. So going back to the call waits for it to be gone.
import { reactive, watch } from "vue";
import type { Router } from "vue-router";
import { goBack } from "./back";
import { pageTransitionsDone } from "./page-transition";

type Screen = {
  path: string;
  position: number | null;
  inHistory: boolean;
};

const screens = reactive<Screen[]>([]);

function historyPosition(): number | null {
  const position = (window.history.state as { position?: unknown } | null)?.position;
  return typeof position === "number" ? position : null;
}

export function callScreenMounted(router?: Router): () => void {
  const position = historyPosition();
  const screen: Screen = reactive({
    path: router?.currentRoute.value.path ?? "",
    position,
    inHistory: Boolean(router) && position !== null,
  });
  screens.push(screen);
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

let going: Promise<void> = Promise.resolve();

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
    // Ionic removes the pages a multi-step jump skips only in a single-stack app, and we have the tabs too.
    // Never while a page is moving either, the call screen was left hidden.
    for (let steps = here - under.position; steps > 0; steps--) {
      await pageTransitionsDone();
      await goBack(router);
    }
    return;
  }
  await callScreenGone();
  await router.push(path);
}
