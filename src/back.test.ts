import { beforeEach, describe, expect, it, vi } from "vitest";
import { nextTick, ref } from "vue";

// Android's back button (found on the phones, 2026-09-28): with a plugin open it left the chat,
// or closed the whole app on the tablet. What is open on top has to close first.
const app = vi.hoisted(() => {
  const state = { handler: null as null | (() => void), registered: 0, unregistered: 0, fail: false };
  return {
    state,
    onBackButtonPress: vi.fn(async (handler: () => void) => {
      if (state.fail) throw new Error("not in Tauri");
      state.registered += 1;
      state.handler = handler;
      return {
        unregister: async () => {
          state.unregistered += 1;
          if (state.handler === handler) state.handler = null;
        },
      };
    }),
  };
});
vi.mock("@tauri-apps/api/app", () => ({ onBackButtonPress: app.onBackButtonPress }));

import { closeAll, closeOnBack, closeOnBackWhile, goBack } from "./back";

const settle = async () => {
  for (let at = 0; at < 5; at += 1) await Promise.resolve();
};
const press = async () => {
  app.state.handler?.();
  await settle();
};

describe("the back button", () => {
  beforeEach(() => {
    Object.assign(app.state, { handler: null, registered: 0, unregistered: 0, fail: false });
  });

  it("keeps its usual job while nothing is open on top", async () => {
    await settle();
    expect(app.state.registered).toBe(0);
  });

  it("closes what was opened last first, then gives the button back", async () => {
    const closed: string[] = [];
    closeOnBack(() => closed.push("apps"));
    closeOnBack(() => closed.push("plugin"));
    await settle();
    expect(app.state.handler).not.toBeNull();

    await press();
    expect(closed).toEqual(["plugin"]);
    await press();
    expect(closed).toEqual(["plugin", "apps"]);
    expect(app.state.handler).toBeNull();
  });

  it("forgets what was closed some other way", async () => {
    const closed: string[] = [];
    closeOnBack(() => closed.push("apps"));
    const release = closeOnBack(() => closed.push("plugin"));
    await settle();
    release();
    await press();
    expect(closed).toEqual(["apps"]);
    expect(app.state.handler).toBeNull();
  });

  // A link opened the app (2026-10-08): whatever is open on top closes, as with Back, and the app
  // goes to the link's page. Then the button does its usual job again.
  it("closes everything open on top at once, the latest first", async () => {
    const closed: string[] = [];
    closeOnBack(() => closed.push("apps"));
    closeOnBack(() => closed.push("plugin"));
    await settle();
    closeAll();
    await settle();
    expect(closed).toEqual(["plugin", "apps"]);
    expect(app.state.handler).toBeNull();
    await press();
    expect(closed).toEqual(["plugin", "apps"]);
  });

  // Seen on the Lenovo tablet (2026-10-08): a tool's own page closes by going back, and that back
  // landed after the link's page was pushed, undoing it. Whoever closes everything waits for it.
  it("finishes closing only once a closer that goes somewhere has got there, one after another", async () => {
    const closed: string[] = [];
    let land!: () => void;
    closeOnBack(() => closed.push("apps"));
    closeOnBack(
      () =>
        new Promise<void>((resolve) => {
          closed.push("page");
          land = () => {
            closed.push("landed");
            resolve();
          };
        }),
    );
    let done = false;
    const closing = closeAll().then(() => (done = true));
    await settle();
    expect(closed).toEqual(["page"]);
    expect(done).toBe(false);

    land();
    await closing;
    expect(closed).toEqual(["page", "landed", "apps"]);
  });

  it("closes nothing when nothing is open", async () => {
    await expect(closeAll()).resolves.toBeUndefined();
    await settle();
    expect(app.state.registered).toBe(0);
  });

  it("does nothing where there is no back button to hear", async () => {
    app.state.fail = true;
    const release = closeOnBack(() => undefined);
    await settle();
    expect(() => release()).not.toThrow();
  });

  it("follows something shown on screen: while it is there, back closes it", async () => {
    const open = ref(false);
    closeOnBackWhile(() => open.value, () => (open.value = false));
    await settle();
    expect(app.state.handler).toBeNull();

    open.value = true;
    await nextTick();
    await settle();
    await press();
    expect(open.value).toBe(false);
    await nextTick();
    await settle();
    expect(app.state.handler).toBeNull();
  });
});

describe("going back", () => {
  /** A router as the app's: going back lands later (the browser's popstate), then says so. */
  function fakeRouter() {
    const after: Array<() => void> = [];
    return {
      after,
      back: vi.fn(),
      afterEach: vi.fn((hook: () => void) => {
        after.push(hook);
        return () => after.splice(after.indexOf(hook), 1);
      }),
      land: () => [...after].forEach((hook) => hook()),
    };
  }

  it("is over only once the router has landed where it went back to", async () => {
    const router = fakeRouter();
    let done = false;
    const going = goBack(router as never).then(() => (done = true));
    expect(router.back).toHaveBeenCalledTimes(1);
    await settle();
    expect(done).toBe(false);

    router.land();
    await going;
    expect(done).toBe(true);
    expect(router.after).toHaveLength(0);
  });
});
