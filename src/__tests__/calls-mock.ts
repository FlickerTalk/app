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
    speaker: false,
    route: null,
    cameraOff: false,
    local: null,
    remote: null,
    native: false,
    view: { available: false, camera: false, paused: false, facing: "front", remote: false, remotePaused: false },
    cameraDenied: false,
    cameraFailed: false,
    presenting: null,
    canPresent: false,
  };
}

export const call = reactive<CallState>(idleCall());
export const history = reactive({ calls: [] as CallEntry[] });
export const actions = {
  startCall: vi.fn(),
  acceptCall: vi.fn(),
  hangUp: vi.fn(),
  toggleMute: vi.fn(),
  toggleSpeaker: vi.fn(),
  toggleCamera: vi.fn(),
  switchCamera: vi.fn(),
  layoutVideo: vi.fn(),
  hideVideo: vi.fn(),
  loadHistory: vi.fn(),
  presentInCall: vi.fn(),
  stopPresenting: vi.fn(),
};

export function resetCalls() {
  Object.assign(call, idleCall());
  history.calls = [];
  Object.values(actions).forEach((action) => action.mockReset());
}

export function callsMock() {
  return { call, history, ...actions, rectOf, cannotPresent };
}

function cannotPresent(error: unknown) {
  return String(error instanceof Error ? error.message : error) === "peer_cannot_present";
}

function rectOf(element: Element | null | undefined) {
  if (!element) return null;
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
}
