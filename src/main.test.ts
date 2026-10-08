import { describe, expect, it, vi } from "vitest";

// main.ts gives Ionic the app's own page transition (2026-10-09, page-transition.ts).
const installed = vi.hoisted(() => [] as unknown[]);

vi.mock("@ionic/vue", async (original) => ({
  ...(await original<typeof import("@ionic/vue")>()),
  IonicVue: { install: (_app: unknown, config: unknown) => installed.push(config) },
}));
vi.mock("./App.vue", () => ({ default: { render: () => null } }));
vi.mock("./router", () => ({ router: { install: () => {}, isReady: () => Promise.resolve() } }));
vi.mock("./core", () => ({ start: () => Promise.resolve(), enablePush: () => Promise.resolve() }));
vi.mock("./calls", () => ({ startCalls: () => Promise.resolve(), loadHistory: () => Promise.resolve() }));
vi.mock("./moving", () => ({ startMoving: () => Promise.resolve() }));
vi.mock("./viewport", () => ({ startViewportFit: () => {} }));

describe("the app's start", () => {
  it("gives Ionic the app's page transition", async () => {
    document.body.innerHTML = `<div id="app"></div>`;
    const { pageTransition } = await import("./page-transition");
    await import("./main");
    await vi.waitFor(() => expect(installed).toHaveLength(1));
    expect(installed[0]).toMatchObject({ navAnimation: pageTransition });
  });
});
