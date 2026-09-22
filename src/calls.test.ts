import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

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

import * as calls from "./calls";
import { setCallRouting } from "./preferences";

class FakeTrack {
  enabled = true;
  stopped = false;
  constructor(public kind: string) {}
  stop() {
    this.stopped = true;
  }
}

class FakeStream {
  tracks: FakeTrack[];
  constructor(video: boolean) {
    this.tracks = [new FakeTrack("audio"), ...(video ? [new FakeTrack("video")] : [])];
  }
  getTracks() {
    return this.tracks;
  }
  getAudioTracks() {
    return this.tracks.filter((track) => track.kind === "audio");
  }
  getVideoTracks() {
    return this.tracks.filter((track) => track.kind === "video");
  }
}

class FakePeer {
  static last: FakePeer;
  added: FakeTrack[] = [];
  remote: RTCSessionDescriptionInit | null = null;
  localDescription: RTCSessionDescriptionInit | null = null;
  iceGatheringState = "complete";
  connectionState = "new";
  closed = false;
  ontrack: ((event: { streams: unknown[] }) => void) | null = null;
  onconnectionstatechange: (() => void) | null = null;
  onicegatheringstatechange: (() => void) | null = null;
  constructor(public config: RTCConfiguration) {
    FakePeer.last = this;
  }
  addTrack(track: FakeTrack) {
    this.added.push(track);
  }
  async createOffer() {
    return { type: "offer" as const, sdp: "offer-sdp" };
  }
  async createAnswer() {
    return { type: "answer" as const, sdp: "answer-sdp" };
  }
  async setLocalDescription(description: RTCSessionDescriptionInit) {
    this.localDescription = description;
  }
  async setRemoteDescription(description: RTCSessionDescriptionInit) {
    this.remote = description;
  }
  close() {
    this.closed = true;
  }
  connect(state: string) {
    this.connectionState = state;
    this.onconnectionstatechange?.();
  }
}

const servers = [
  { urls: ["stun:stun.example:3478"] },
  { urls: ["turn:turn.example:3478"], username: "u", credential: "c" },
];
let stream: FakeStream;

function incoming(video = true) {
  tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", kind: "incoming", video, sdp: "their-offer" } });
}

describe("calls", () => {
  beforeEach(async () => {
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_call_ice" ? servers : command === "core_call_start" ? "call-1" : command === "core_calls" ? [] : undefined),
    );
    calls.media.getUserMedia = vi.fn(async (constraints: MediaStreamConstraints) => {
      stream = new FakeStream(Boolean(constraints.video));
      return stream as unknown as MediaStream;
    });
    calls.media.createPeer = (config: RTCConfiguration) => new FakePeer(config) as unknown as RTCPeerConnection;
    localStorage.clear();
    calls.reset();
    await calls.startCalls();
  });

  afterEach(() => vi.useRealTimers());

  it("calls with the camera and microphone and the gathered offer", async () => {
    await calls.startCall("ft_bob", true);
    expect(calls.media.getUserMedia).toHaveBeenCalledWith({ audio: true, video: { facingMode: "user" } });
    expect(FakePeer.last.added.map((track) => track.kind)).toEqual(["audio", "video"]);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start", { contact: "ft_bob", video: true, sdp: "offer-sdp" });
    expect(calls.call).toMatchObject({ id: "call-1", contact: "ft_bob", outgoing: true, video: true, phase: "calling" });
  });

  it("uses no camera on a voice call", async () => {
    await calls.startCall("ft_bob", false);
    expect(calls.media.getUserMedia).toHaveBeenCalledWith({ audio: true, video: false });
  });

  // §17: the routing chosen in Settings decides whether the relay may be used.
  it("follows the call routing of the settings", async () => {
    await calls.startCall("ft_bob", false);
    expect(FakePeer.last.config).toEqual({ iceServers: servers });
    calls.reset();
    setCallRouting("always");
    await calls.startCall("ft_bob", false);
    expect(FakePeer.last.config).toEqual({ iceServers: [servers[1]], iceTransportPolicy: "relay" });
    calls.reset();
    setCallRouting("direct");
    await calls.startCall("ft_bob", false);
    expect(FakePeer.last.config).toEqual({ iceServers: [servers[0]] });
  });

  it("connects once the contact answers", async () => {
    await calls.startCall("ft_bob", true);
    tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", kind: "answered", sdp: "their-answer" } });
    await flushPromises();
    expect(FakePeer.last.remote).toEqual({ type: "answer", sdp: "their-answer" });
    expect(calls.call.phase).toBe("connecting");
    FakePeer.last.connect("connected");
    expect(calls.call.phase).toBe("active");
    expect(calls.call.since).toBeGreaterThan(0);
  });

  it("rings for an incoming call and answers it", async () => {
    incoming();
    expect(calls.call).toMatchObject({ id: "call-1", contact: "ft_bob", outgoing: false, video: true, phase: "ringing" });
    await calls.acceptCall();
    expect(FakePeer.last.remote).toEqual({ type: "offer", sdp: "their-offer" });
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer", { call: "call-1", sdp: "answer-sdp" });
    expect(calls.call.phase).toBe("connecting");
  });

  it("declines an incoming call", async () => {
    incoming();
    await calls.hangUp();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "call-1", failed: false });
    expect(calls.call.phase).toBe("ended");
  });

  it("ends when the contact ends it, and lets go of the camera", async () => {
    await calls.startCall("ft_bob", true);
    tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", kind: "ended", outcome: "declined" } });
    await flushPromises();
    expect(calls.call).toMatchObject({ phase: "ended", outcome: "declined" });
    expect(FakePeer.last.closed).toBe(true);
    expect(stream.tracks.every((track) => track.stopped)).toBe(true);
  });

  it("tells the contact when the media cannot connect", async () => {
    await calls.startCall("ft_bob", false);
    FakePeer.last.connect("failed");
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "call-1", failed: true });
    expect(calls.call.phase).toBe("ended");
  });

  it("mutes the microphone and turns the camera off", async () => {
    await calls.startCall("ft_bob", true);
    calls.toggleMute();
    calls.toggleCamera();
    expect(calls.call).toMatchObject({ muted: true, cameraOff: true });
    expect(stream.getAudioTracks()[0].enabled).toBe(false);
    expect(stream.getVideoTracks()[0].enabled).toBe(false);
  });

  it("ignores what happens to other calls", async () => {
    await calls.startCall("ft_bob", false);
    tauri.handlers["ft://call"]({ payload: { contact: "ft_carol", call: "other", kind: "ended", outcome: "missed" } });
    await flushPromises();
    expect(calls.call.phase).toBe("calling");
  });

  it("gives up when nobody answers", async () => {
    vi.useFakeTimers();
    await calls.startCall("ft_bob", false);
    await vi.advanceTimersByTimeAsync(calls.RING_LIMIT + 1);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "call-1", failed: false });
  });

  // Without a camera or microphone (permission denied, say) there is no call to hang in the air.
  it("ends as failed when the call cannot start", async () => {
    calls.media.getUserMedia = vi.fn(async () => {
      throw new Error("denied");
    });
    await calls.startCall("ft_bob", true);
    expect(calls.call).toMatchObject({ phase: "ended", outcome: "failed" });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_start", expect.anything());
  });

  it("keeps the history up to date", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_calls"
          ? [{ id: "c9", contact: "ft_bob", name: "Bob", outgoing: false, video: true, startedAt: 1, seconds: 0, outcome: "missed" }]
          : undefined,
      ),
    );
    tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "c9", kind: "ended", outcome: "missed" } });
    await flushPromises();
    expect(calls.history.calls.map((entry) => entry.id)).toEqual(["c9"]);
  });
});
