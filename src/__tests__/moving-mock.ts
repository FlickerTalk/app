/**
 * A stand-in for `src/moving.ts` in view tests: the real reactive state, spies for the actions.
 * Use with `vi.mock("../moving", () => movingMock())`.
 */
import { reactive } from "vue";
import { vi } from "vitest";
import type { MovePhase } from "../moving";

export const move = reactive({ phase: "idle" as MovePhase, done: 0, total: 0, error: "" });
export const actions = {
  invite: vi.fn(() => Promise.resolve("https://flickertalk.com/move#abc")),
  moveTo: vi.fn(() => Promise.resolve()),
};

export function resetMoving() {
  Object.assign(move, { phase: "idle", done: 0, total: 0, error: "" });
  actions.invite.mockClear();
  actions.moveTo.mockClear();
}

export function movingMock() {
  return { move, ...actions };
}
