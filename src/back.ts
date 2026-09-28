// Android's back button (2026-09-28): while something is open on top (a plugin, the apps, the
// actions of a message), the button closes it, the latest first. With nothing open, nobody
// listens and the button does its usual job (the previous page, or leaving the app).
import { onScopeDispose, watch } from "vue";
import { onBackButtonPress } from "@tauri-apps/api/app";

type Listener = { unregister: () => Promise<void> };

const open: Array<() => void> = [];
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
  close?.();
}

/** Back closes this until it is released (it closed some other way). */
export function closeOnBack(close: () => void): () => void {
  open.push(close);
  sync();
  return () => {
    const at = open.lastIndexOf(close);
    if (at >= 0) open.splice(at, 1);
    sync();
  };
}

/** Back closes what `shown` says is on screen, for as long as it is; for a component's overlays. */
export function closeOnBackWhile(shown: () => boolean, close: () => void): void {
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
