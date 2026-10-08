import { beforeEach, describe, expect, it, vi } from "vitest";
import { ref } from "vue";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    tauri.handlers[name] = handler;
    return Promise.resolve(() => undefined);
  },
}));
const back = vi.hoisted(() => ({ closeAll: vi.fn() }));
vi.mock("./back", () => back);

import { decide, checkOpenedLink, isAppLink, openInApp, releaseKept, startOpenedLinks, takeOpened, type OpenedLink } from "./opened";
import { move, reset as resetMove } from "./moving";
import { clearOnboarded, setOnboarded } from "./preferences";

const contact: OpenedLink = { kind: "add", link: "https://flickertalk.com/add#card", valid: true };
const broken: OpenedLink = { kind: "add", link: "https://flickertalk.com/add#cut", valid: false };
const invite: OpenedLink = { kind: "move", link: "https://flickertalk.com/move#invite", valid: true };

/** A router as the app's: where it is, where it goes, and what it says after each navigation. */
function fakeRouter(path = "/tabs/chats") {
  const after: Array<(to: { path: string }) => void> = [];
  const currentRoute = ref({ path });
  const push = vi.fn(async (to: string) => {
    currentRoute.value = { path: to.split("?")[0] };
    after.forEach((hook) => hook(currentRoute.value));
  });
  return {
    currentRoute,
    push,
    afterEach: (hook: (to: { path: string }) => void) => {
      after.push(hook);
      return () => after.splice(after.indexOf(hook), 1);
    },
    /** The user goes somewhere by themselves. */
    goTo(to: string) {
      currentRoute.value = { path: to };
      after.forEach((hook) => hook(currentRoute.value));
    },
  };
}

let stop: (() => void) | undefined;
function start(router = fakeRouter()) {
  stop?.();
  stop = startOpenedLinks(router as never);
  return router;
}

/** The core says the phone opened the app with `link` (once), then nothing. */
function opensWith(link: OpenedLink | null) {
  let given = link;
  tauri.invoke.mockImplementation(async (command: string, args?: { url?: string }) => {
    if (command === "core_opened_link") {
      const once = given;
      given = null;
      return once;
    }
    if (command === "core_read_link") return args?.url === contact.link ? contact : null;
    return undefined;
  });
}

const settle = async () => {
  for (let at = 0; at < 10; at += 1) await Promise.resolve();
};

// Links that open the app (2026-10-08, docs/plan-enlaces-app.md): a link never acts alone. It
// opens its page with the field filled, and the user taps.
describe("deciding what an opened link does", () => {
  const app = { onboarded: true, moving: false, onCallScreen: false };

  it("goes to Add contact with a contact link, and to the move screen with an invite", () => {
    expect(decide(contact, app)).toBe("add");
    expect(decide(broken, app)).toBe("add");
    expect(decide(invite, app)).toBe("move");
  });

  it("keeps a contact link on the first run until Start, and drops an invite there", () => {
    expect(decide(contact, { ...app, onboarded: false })).toBe("keep");
    expect(decide(invite, { ...app, onboarded: false })).toBe("drop");
  });

  it("drops any link while a move is going on", () => {
    expect(decide(contact, { ...app, moving: true })).toBe("drop");
    expect(decide(invite, { ...app, moving: true })).toBe("drop");
    expect(decide(contact, { ...app, onboarded: false, moving: true })).toBe("drop");
  });

  it("waits while the call screen is on", () => {
    expect(decide(contact, { ...app, onCallScreen: true })).toBe("wait");
    expect(decide(invite, { ...app, onCallScreen: true })).toBe("wait");
  });
});

describe("the app's own links", () => {
  it("are only the exact add and move addresses", () => {
    expect(isAppLink("https://flickertalk.com/add#card")).toBe(true);
    expect(isAppLink("https://flickertalk.com/move#invite")).toBe(true);
    for (const other of [
      "https://flickertalk.com/add",
      "https://flickertalk.com/games/chess",
      "https://www.flickertalk.com/add#card",
      "http://flickertalk.com/add#card",
      "flickertalk://add#card",
      "https://example.com/?https://flickertalk.com/add#card",
    ]) {
      expect(isAppLink(other), other).toBe(false);
    }
  });
});

describe("an opened link", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    back.closeAll.mockClear();
    localStorage.clear();
    setOnboarded();
    resetMove();
    takeOpened("add");
    takeOpened("move");
    releaseKept();
  });

  it("opens Add contact with its link, which the page takes once", async () => {
    const router = start();
    opensWith(contact);
    await checkOpenedLink();
    expect(router.push).toHaveBeenCalledWith("/add-contact");
    expect(takeOpened("add")).toEqual(contact);
    expect(takeOpened("add")).toBeNull();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_add_contact", expect.anything());
  });

  it("opens the move screen of the old phone with an invite", async () => {
    const router = start();
    opensWith(invite);
    await checkOpenedLink();
    expect(router.push).toHaveBeenCalledWith("/move");
    expect(takeOpened("move")).toEqual(invite);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_move_to", expect.anything());
  });

  it("closes a plugin, a sheet or the scanner first, as Back would", async () => {
    const router = start();
    opensWith(contact);
    await checkOpenedLink();
    expect(back.closeAll).toHaveBeenCalledTimes(1);
    expect(back.closeAll.mock.invocationCallOrder[0]).toBeLessThan(router.push.mock.invocationCallOrder[0]);
  });

  it("does nothing when the app opened with no link", async () => {
    const router = start();
    opensWith(null);
    await checkOpenedLink();
    expect(router.push).not.toHaveBeenCalled();
    expect(back.closeAll).not.toHaveBeenCalled();
  });

  it("is asked for again when the app hears one came", async () => {
    const router = start();
    await settle();
    opensWith(contact);
    tauri.handlers["ft://opened-link"]({ payload: null });
    await settle();
    expect(router.push).toHaveBeenCalledWith("/add-contact");
  });

  it("waits on the first run until Start, then opens Add contact", async () => {
    clearOnboarded();
    const router = start(fakeRouter("/welcome"));
    opensWith(contact);
    await checkOpenedLink();
    expect(router.push).not.toHaveBeenCalled();
    setOnboarded();
    await releaseKept();
    expect(router.push).toHaveBeenCalledWith("/add-contact");
    expect(takeOpened("add")).toEqual(contact);
  });

  it("drops an invite on a phone with no identity yet (the new phone)", async () => {
    clearOnboarded();
    const router = start(fakeRouter("/welcome"));
    opensWith(invite);
    await checkOpenedLink();
    setOnboarded();
    await releaseKept();
    expect(router.push).not.toHaveBeenCalled();
  });

  it("drops anything while a move is going on", async () => {
    const router = start(fakeRouter("/move"));
    move.phase = "moving";
    opensWith(contact);
    await checkOpenedLink();
    move.phase = "waiting";
    opensWith(contact);
    await checkOpenedLink();
    expect(router.push).not.toHaveBeenCalled();
  });

  it("waits until the call screen is left", async () => {
    const router = start(fakeRouter("/call/ft_bob123456789"));
    opensWith(contact);
    await checkOpenedLink();
    expect(router.push).not.toHaveBeenCalled();
    expect(back.closeAll).not.toHaveBeenCalled();
    router.goTo("/chat/ft_bob123456789");
    await settle();
    expect(router.push).toHaveBeenCalledWith("/add-contact");
    expect(takeOpened("add")).toEqual(contact);
  });

  // Inside a chat, a FlickerTalk link is the app's own: it never goes out to the system.
  it("is handled in the app when tapped inside a chat", async () => {
    const router = start(fakeRouter("/chat/ft_bob123456789"));
    opensWith(null);
    expect(await openInApp(contact.link)).toBe(true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_read_link", { url: contact.link });
    expect(router.push).toHaveBeenCalledWith("/add-contact");
    expect(await openInApp("https://flickertalk.com/add")).toBe(false);
  });
});
