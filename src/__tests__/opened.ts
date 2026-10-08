/**
 * The phone opens the app with a link (2026-10-08), for the tests of the pages that take it: the
 * bridge answers `core_opened_link` once with it, and `opened.ts` hands it to its page as the app
 * would. Whatever else the bridge answers stays as it was.
 */
import { checkOpenedLink, startOpenedLinks, type OpenedLink } from "../opened";
import { setOnboarded } from "../preferences";

type Internals = { invoke: (command: string, args?: Record<string, unknown>) => Promise<unknown> };

export async function linkOpens(opened: OpenedLink, at = "/tabs/chats"): Promise<void> {
  setOnboarded();
  const internals = (window as unknown as { __TAURI_INTERNALS__: Internals }).__TAURI_INTERNALS__;
  const before = internals.invoke;
  internals.invoke = (command, args) => (command === "core_opened_link" ? Promise.resolve(opened) : before(command, args));
  const router = { currentRoute: { value: { path: at } }, push: async () => undefined, afterEach: () => () => undefined };
  const stop = startOpenedLinks(router as never);
  try {
    await checkOpenedLink();
  } finally {
    internals.invoke = before;
    stop();
  }
}
