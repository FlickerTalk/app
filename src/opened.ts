/**
 * Links that open the app (2026-10-08; App Links on Android, Universal Links on iOS):
 * `https://flickertalk.com/add#…` and `/move#…`. The core says which page a link leads to
 * (`core_opened_link`, read once; `core_read_link` for one tapped inside a chat) and this decides
 * what to do with it. **A link never acts alone**: it opens its page with the field filled, and
 * nothing is added or moved until the user taps. A contact link always goes to the main list,
 * never to a hidden session. There is no `flickertalk://` scheme: any app could claim one.
 */
import { reactive } from "vue";
import type { Router } from "vue-router";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { closeAll } from "./back";
import { move } from "./moving";
import { isOnboarded } from "./preferences";

export interface OpenedLink {
  kind: "add" | "move";
  link: string;
  /** Whether it reads as a card or an invite; a broken one still opens its page, which says so. */
  valid: boolean;
}

/** Where the app is when a link comes. */
export interface Situation {
  onboarded: boolean;
  /** A move to or from another phone is going on (§60). */
  moving: boolean;
  onCallScreen: boolean;
}

/** Go to its page, keep it until Start (first run), wait until the call screen is left, or drop it. */
export type Decision = "add" | "move" | "keep" | "wait" | "drop";

/** The event the core sends when a link opens the app that runs already. */
export const OPENED_LINK_EVENT = "ft://opened-link";

/** The two links the app handles itself, exactly as they start. */
const PREFIXES = ["https://flickertalk.com/add#", "https://flickertalk.com/move#"];

export function decide(opened: OpenedLink, now: Situation): Decision {
  if (now.moving) return "drop";
  // An invite is for the old phone: one with no identity (the new one) has nothing to move.
  if (opened.kind === "move" && !now.onboarded) return "drop";
  if (now.onCallScreen) return "wait";
  if (!now.onboarded) return "keep";
  return opened.kind;
}

/** Whether a link is one the app opens itself (a tap inside a chat). */
export function isAppLink(href: string): boolean {
  return PREFIXES.some((prefix) => href.startsWith(prefix));
}

/** What each page takes to fill its field, once. */
const handed = reactive<{ add: OpenedLink | null; move: OpenedLink | null }>({ add: null, move: null });

/** The link handed to the page of `kind`, if any; it goes with the answer. */
export function takeOpened(kind: OpenedLink["kind"]): OpenedLink | null {
  const taken = handed[kind];
  handed[kind] = null;
  return taken;
}

/** Whether a link waits for the page of `kind`: the page watches this while it is open. */
export function waitingFor(kind: OpenedLink["kind"]): boolean {
  return handed[kind] !== null;
}

let router: Router | undefined;
/** A contact link that came on the first run, until Start. */
let kept: OpenedLink | null = null;
/** A link that came during a call, until the call screen is left. */
let waiting: OpenedLink | null = null;

function onCallScreen(path: string): boolean {
  return path.startsWith("/call/");
}

function situation(): Situation {
  return {
    onboarded: isOnboarded(),
    moving: ["waiting", "moving", "received", "sent"].includes(move.phase),
    onCallScreen: onCallScreen(router?.currentRoute.value.path ?? ""),
  };
}

async function act(opened: OpenedLink | null): Promise<void> {
  if (!opened || !router) return;
  const decision = decide(opened, situation());
  switch (decision) {
    case "drop":
      return;
    case "keep":
      kept = opened;
      return;
    case "wait":
      waiting = opened;
      return;
    case "add":
    case "move":
      // Whatever is open on top closes, as with Back: the page must not open under a plugin. A tool's
      // own page closes by going back, so the push waits until it got there.
      await closeAll();
      handed[decision] = opened;
      await router.push(decision === "add" ? "/add-contact" : "/move");
  }
}

/** Asks the core whether a link opened the app, and acts on it. */
export async function checkOpenedLink(): Promise<void> {
  await act(await invoke<OpenedLink | null>("core_opened_link").catch(() => null));
}

/** A FlickerTalk link tapped inside a chat: `false` if it is none of the app's. */
export async function openInApp(url: string): Promise<boolean> {
  const read = await invoke<OpenedLink | null>("core_read_link", { url }).catch(() => null);
  if (!read) return false;
  await act(read);
  return true;
}

/** Start was tapped (first run): the contact link that came meanwhile opens its page now. */
export async function releaseKept(): Promise<void> {
  const link = kept;
  kept = null;
  await act(link);
}

/** The app follows opened links from now on (`App.vue`); returns how to stop. */
export function startOpenedLinks(appRouter: Router): () => void {
  router = appRouter;
  kept = null;
  waiting = null;
  const stopAfter = appRouter.afterEach((to) => {
    if (!waiting || onCallScreen(to.path)) return;
    const link = waiting;
    waiting = null;
    void act(link);
  });
  let unlisten: (() => void) | undefined;
  let stopped = false;
  void listen(OPENED_LINK_EVENT, () => void checkOpenedLink())
    .then((stop) => (stopped ? stop() : (unlisten = stop)))
    .catch(() => undefined);
  return () => {
    stopped = true;
    stopAfter();
    // Where the event plugin is gone (a test, a page being torn down) there is nothing to stop.
    void Promise.resolve()
      .then(() => unlisten?.())
      .catch(() => undefined);
    if (router === appRouter) router = undefined;
  };
}
