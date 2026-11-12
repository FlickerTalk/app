import { onScopeDispose, watch } from "vue";
import { onBackButtonPress } from "@tauri-apps/api/app";
import type { Router } from "vue-router";

type Listener = { unregister: () => Promise<void> };
export type CloseReason = "back" | "all";
type Close = (why: CloseReason) => unknown;

const open: Array<Close> = [];
let listening: Promise<Listener | undefined> | undefined;

// a listener takes the button over entirely
function sync(): void {
  if (open.length && !listening) {
    // no back button outside Tauri
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
  void close?.("back");
}

// each closer finishes before the next, so a back that lands late can't undo the caller's push
export async function closeAll(): Promise<void> {
  const closing = open.splice(0).reverse();
  sync();
  for (const close of closing) await close("all");
}

// popstate comes after the router has landed
export function goBack(router: Router): Promise<void> {
  return new Promise((resolve) => {
    const stop = router.afterEach(() => {
      stop();
      resolve();
    });
    router.back();
  });
}

export function closeOnBack(close: Close): () => void {
  open.push(close);
  sync();
  return () => {
    const at = open.lastIndexOf(close);
    if (at >= 0) open.splice(at, 1);
    sync();
  };
}

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
