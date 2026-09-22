/**
 * A stand-in for the bridge between the WebView and Rust (`window.__TAURI_INTERNALS__`), so the
 * real `@tauri-apps/api` code runs in unit tests. `answer` decides what each command returns.
 */
export type Answer = (command: string, args?: Record<string, unknown>) => unknown;

const LISTS = new Set(["core_conversations", "core_messages"]);

export const emptyAnswers: Answer = (command) => (LISTS.has(command) ? [] : undefined);

export function installTauri(answer: Answer = emptyAnswers): void {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: (command: string, args?: Record<string, unknown>) => Promise.resolve(answer(command, args)),
    transformCallback: () => 0,
    unregisterCallback: () => undefined,
  };
}
