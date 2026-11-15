import { beforeAll, describe, expect, it, vi } from "vitest";
import { setOnboarded } from "./preferences";

const installed = vi.hoisted(() => [] as unknown[]);
const supportAdds = vi.hoisted(() => [] as true[]);

vi.mock("@ionic/vue", () => ({
  IonicVue: {
    install: (_app: unknown, config: unknown) => installed.push(config),
  },
  isPlatform: () => false,
}));
vi.mock("./App.vue", () => ({ default: { render: () => null } }));
vi.mock("./router", () => ({
  router: { install: () => {}, isReady: () => Promise.resolve() },
}));
vi.mock("./core", () => ({
  start: () => Promise.resolve(),
  enablePush: () => Promise.resolve(),
  addSupportContact: () => Promise.resolve(supportAdds.push(true)),
}));
vi.mock("./calls", () => ({
  startCalls: () => Promise.resolve(),
  loadHistory: () => Promise.resolve(),
}));
vi.mock("./moving", () => ({ startMoving: () => Promise.resolve() }));
vi.mock("./viewport", () => ({ startViewportFit: () => {} }));

describe("the app's start", () => {
  beforeAll(() => setOnboarded());

  it("gives Ionic the app's page transition", async () => {
    document.body.innerHTML = `<div id="app"></div>`;
    const { pageTransition } = await import("./page-transition");
    await import("./main");
    await vi.waitFor(() => expect(installed).toHaveLength(1));
    expect(installed[0]).toMatchObject({ navAnimation: pageTransition });
  });

  it("adds the support contact for someone who is past the welcome", async () => {
    await import("./main");
    await vi.waitFor(() => expect(supportAdds).toHaveLength(1));
  });
});
