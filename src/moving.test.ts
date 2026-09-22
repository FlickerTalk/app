import { beforeEach, describe, expect, it, vi } from "vitest";

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

import * as moving from "./moving";
import { isOnboarded, setOnboarded } from "./preferences";

const event = (payload: unknown) => tauri.handlers["ft://move"]({ payload });

// §60: moving to a new phone, as the screens follow it.
describe("moving", () => {
  beforeEach(async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_move_invite" ? "https://flickertalk.com/move#abc" : undefined),
    );
    localStorage.clear();
    moving.reset();
    await moving.startMoving();
  });

  it("shows the new phone's invite and waits", async () => {
    expect(await moving.invite()).toBe("https://flickertalk.com/move#abc");
    expect(moving.move.phase).toBe("waiting");
  });

  it("hands the old phone's identity to the scanned invite", async () => {
    await moving.moveTo("  https://flickertalk.com/move#abc ");
    expect(tauri.invoke).toHaveBeenCalledWith("core_move_to", { link: "https://flickertalk.com/move#abc" });
    expect(moving.move.phase).toBe("moving");
  });

  it("says why the move could not start", async () => {
    tauri.invoke.mockRejectedValueOnce("the new phone cannot be reached directly");
    await moving.moveTo("https://flickertalk.com/move#abc");
    expect(moving.move).toMatchObject({ phase: "failed", error: "the new phone cannot be reached directly" });
  });

  it("follows the progress", () => {
    event({ kind: "progress", done: 3, total: 12 });
    expect(moving.move).toMatchObject({ phase: "moving", done: 3, total: 12 });
  });

  // The app starts again on its own: the new phone must open on the chats, not the welcome.
  it("the new phone skips the welcome once it has the identity", () => {
    event({ kind: "received" });
    expect(moving.move.phase).toBe("received");
    expect(isOnboarded()).toBe(true);
  });

  // The old phone is erased: it starts again on the welcome, as a new install.
  it("the old phone starts over", () => {
    setOnboarded();
    event({ kind: "sent" });
    expect(moving.move.phase).toBe("sent");
    expect(isOnboarded()).toBe(false);
  });

  it("tells when the copy did not arrive whole", () => {
    event({ kind: "failed" });
    expect(moving.move.phase).toBe("failed");
  });
});
