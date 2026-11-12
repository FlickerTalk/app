export type Answer = (command: string, args?: Record<string, unknown>) => unknown;

const LISTS = new Set(["core_conversations", "core_messages"]);

export const emptyAnswers: Answer = (command) => (LISTS.has(command) ? [] : undefined);

export function installTauri(answer: Answer = emptyAnswers): void {
  (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
    invoke: (command: string, args?: Record<string, unknown>) => Promise.resolve(answer(command, args)),
    convertFileSrc: (path: string, protocol = "asset") => `http://${protocol}.localhost/${path}`,
    transformCallback: () => 0,
    unregisterCallback: () => undefined,
    unregisterListener: () => undefined,
  };
}
