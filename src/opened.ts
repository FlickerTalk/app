// a link never acts on its own, the user still taps. No flickertalk:// scheme, any app could claim it.
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
  valid: boolean;
}

export interface Situation {
  onboarded: boolean;
  moving: boolean;
  onCallScreen: boolean;
}

export type Decision = "add" | "move" | "keep" | "wait" | "drop";

export const OPENED_LINK_EVENT = "ft://opened-link";

const PREFIXES = ["https://flickertalk.com/add#", "https://flickertalk.com/move#"];

export function decide(opened: OpenedLink, now: Situation): Decision {
  if (now.moving) return "drop";
  if (opened.kind === "move" && !now.onboarded) return "drop";
  if (now.onCallScreen) return "wait";
  if (!now.onboarded) return "keep";
  return opened.kind;
}

export function isAppLink(href: string): boolean {
  return PREFIXES.some((prefix) => href.startsWith(prefix));
}

const handed = reactive<{ add: OpenedLink | null; move: OpenedLink | null }>({ add: null, move: null });

export function takeOpened(kind: OpenedLink["kind"]): OpenedLink | null {
  const taken = handed[kind];
  handed[kind] = null;
  return taken;
}

export function waitingFor(kind: OpenedLink["kind"]): boolean {
  return handed[kind] !== null;
}

let router: Router | undefined;
let kept: OpenedLink | null = null;
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
      // a tool page closes by going back, so wait for closeAll before pushing
      await closeAll();
      handed[decision] = opened;
      await router.push(decision === "add" ? "/add-contact" : "/move");
  }
}

export async function checkOpenedLink(): Promise<void> {
  await act(await invoke<OpenedLink | null>("core_opened_link").catch(() => null));
}

export async function openInApp(url: string): Promise<boolean> {
  const read = await invoke<OpenedLink | null>("core_read_link", { url }).catch(() => null);
  if (!read) return false;
  await act(read);
  return true;
}

export async function releaseKept(): Promise<void> {
  const link = kept;
  kept = null;
  await act(link);
}

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
    // no event plugin in tests
    void Promise.resolve()
      .then(() => unlisten?.())
      .catch(() => undefined);
    if (router === appRouter) router = undefined;
  };
}
