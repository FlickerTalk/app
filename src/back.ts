// Android's back button (2026-09-28): while something is open on top (a plugin, the apps, the
// actions of a message), the button closes it, the latest first. With nothing open, nobody
// listens and the button does its usual job (the previous page, or leaving the app).
import { onScopeDispose, watch } from "vue";
import { onBackButtonPress } from "@tauri-apps/api/app";
import type { Router } from "vue-router";

type Listener = { unregister: () => Promise<void> };
/** Closes something; one that goes somewhere (a page that goes back) is over when it gets there. */
type Close = () => unknown;

const open: Array<Close> = [];
let listening: Promise<Listener | undefined> | undefined;

/** Listens only while there is something to close: a listener takes the button over entirely. */
function sync(): void {
  if (open.length && !listening) {
    // Outside Tauri, or where there is no back button, there is nothing to hear.
    listening = onBackButtonPress(pressed).catch(() => undefined);
  } else if (!open.length && listening) {
    const old = listening;
    listening = undefined;
    void old.then((listener) => listener?.unregister()).catch(() => undefined);
  }
}

function pressed(): void {
  const close = open.pop();
  sync();
  void close?.();
}

/**
 * Closes everything open on top, the latest first, as presses of Back would (2026-10-08: a link
 * opened the app, and its page must not open under a plugin or a sheet). Each closer is over before
 * the next runs, and all of them before this is: a page that closes by going back gets there first,
 * so whatever the caller pushes next is not undone by that back landing after it (the Lenovo
 * tablet, 2026-10-08).
 */
export async function closeAll(): Promise<void> {
  const closing = open.splice(0).reverse();
  sync();
  for (const close of closing) await close();
}

/** Goes back, and is over once the router has landed (the browser's popstate comes later). */
export function goBack(router: Router): Promise<void> {
  return new Promise((resolve) => {
    const stop = router.afterEach(() => {
      stop();
      resolve();
    });
    router.back();
  });
}

/** Back closes this until it is released (it closed some other way). */
export function closeOnBack(close: Close): () => void {
  open.push(close);
  sync();
  return () => {
    const at = open.lastIndexOf(close);
    if (at >= 0) open.splice(at, 1);
    sync();
  };
}

/** Back closes what `shown` says is on screen, for as long as it is; for a component's overlays. */
export function closeOnBackWhile(shown: () => boolean, close: Close): void {
  let release: (() => void) | undefined;
  watch(
    () => Boolean(shown()),
    (now) => {
      release?.();
      release = now ? closeOnBack(close) : undefined;
    },
    { immediate: true },
  );
  onScopeDispose(() => release?.());
}
