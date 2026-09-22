/**
 * A stand-in for `src/calls.ts` in view tests: the real reactive state, spies for the actions.
 * Use with `vi.mock("../calls", () => callsMock())`.
 */
import { reactive } from "vue";
import { vi } from "vitest";
import type { CallEntry, CallState } from "../calls";

export function idleCall(): CallState {
  return {
    id: "",
    contact: "",
    video: false,
    outgoing: false,
    phase: "idle",
    outcome: null,
    since: 0,
    muted: false,
    cameraOff: false,
    local: null,
    remote: null,
  };
}

export const call = reactive<CallState>(idleCall());
export const history = reactive({ calls: [] as CallEntry[] });
export const actions = {
  startCall: vi.fn(),
  acceptCall: vi.fn(),
  hangUp: vi.fn(),
  toggleMute: vi.fn(),
  toggleCamera: vi.fn(),
  loadHistory: vi.fn(),
};

export function resetCalls() {
  Object.assign(call, idleCall());
  history.calls = [];
  Object.values(actions).forEach((action) => action.mockReset());
}

export function callsMock() {
  return { call, history, ...actions };
}
