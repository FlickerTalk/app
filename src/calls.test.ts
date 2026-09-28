import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
// Answering from the notification must open the call screen, as the in-app button does.
const navigation = vi.hoisted(() => ({ push: vi.fn() }));
vi.mock("./router", () => ({ router: { push: navigation.push } }));
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
    calls.media.ringback = { start: vi.fn(), stop: vi.fn() };
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

  // The caller hears that it rings, until the contact answers or it ends.
  it("plays the ringback tone while calling", async () => {
    await calls.startCall("ft_bob", false);
    expect(calls.media.ringback.start).toHaveBeenCalled();
    tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", kind: "answered", sdp: "a" } });
    await flushPromises();
    expect(calls.media.ringback.stop).toHaveBeenCalled();
  });

  it("stops the ringback tone when the call ends before an answer", async () => {
    await calls.startCall("ft_bob", false);
    await calls.hangUp();
    expect(calls.media.ringback.stop).toHaveBeenCalled();
  });

  // Each call starts with the microphone and camera on, whatever the last one ended with.
  it("starts every call unmuted", async () => {
    await calls.startCall("ft_bob", true);
    calls.toggleMute();
    await calls.hangUp();
    incoming();
    expect(calls.call).toMatchObject({ muted: false, cameraOff: false });
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

  // §66: the call notification has Answer and Decline; what the user pressed there reaches the
  // app when it opens.
  it("answers the call the user accepted on the notification", async () => {
    incoming(false);
    await flushPromises();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_pending_call" ? "answer" : command === "core_call_ice" ? servers : command === "core_calls" ? [] : undefined,
      ),
    );

    navigation.push.mockClear();
    await calls.applyCallNotification();
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer", expect.objectContaining({ call: "call-1" }));
    // Without the call screen there is no way to hang up.
    expect(navigation.push).toHaveBeenCalledWith(`/call/${calls.call.contact}`);
  });

  it("ends the call the user declined on the notification", async () => {
    incoming(false);
    await flushPromises();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_pending_call" ? "decline" : command === "core_calls" ? [] : undefined),
    );

    await calls.applyCallNotification();
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "call-1", failed: false });
    expect(calls.call.phase).toBe("ended");
  });

  it("does nothing when the notification was not pressed", async () => {
    incoming(false);
    await flushPromises();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_pending_call" ? "" : command === "core_calls" ? [] : undefined),
    );

    await calls.applyCallNotification();
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_end", expect.anything());
    expect(calls.call.phase).toBe("ringing");
  });
});

// 2026-09-28: on the phones a voice call's media runs in the Rust core (CallKit on a locked
// iPhone has no WebView). The WebView only shows the call; video calls keep the WebView's media.
describe("native voice calls", () => {
  let current: Record<string, unknown> | null;
  let pending: string;

  function answers(command: string) {
    switch (command) {
      case "core_native_calls":
        return true;
      case "core_call_start_native":
        return "call-1";
      case "core_current_call":
        return current;
      case "core_pending_call":
        return pending;
      case "core_call_ice":
        return servers;
      case "core_calls":
        return [];
      default:
        return undefined;
    }
  }

  const event = (payload: Record<string, unknown>) => tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", ...payload } });

  beforeEach(async () => {
    current = null;
    pending = "";
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(answers(command)));
    calls.media.getUserMedia = vi.fn(async () => new FakeStream(false) as unknown as MediaStream);
    calls.media.createPeer = vi.fn((config: RTCConfiguration) => new FakePeer(config) as unknown as RTCPeerConnection);
    calls.media.ringback = { start: vi.fn(), stop: vi.fn() };
    navigation.push.mockClear();
    localStorage.clear();
    calls.reset();
    await calls.startCalls();
  });

  it("places a voice call in the core, with the routing of the settings and no WebView media", async () => {
    setCallRouting("always");
    await calls.startCall("ft_bob", false);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start_native", { contact: "ft_bob", routing: "always" });
    expect(calls.media.getUserMedia).not.toHaveBeenCalled();
    expect(calls.media.createPeer).not.toHaveBeenCalled();
    expect(calls.call).toMatchObject({ id: "call-1", phase: "calling", outgoing: true, video: false });
    expect(calls.media.ringback.start).toHaveBeenCalled();
  });

  it("keeps video calls on the WebView", async () => {
    await calls.startCall("ft_bob", true);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_start_native", expect.anything());
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start", expect.objectContaining({ contact: "ft_bob", video: true }));
  });

  it("goes live when the core says the media connected", async () => {
    await calls.startCall("ft_bob", false);
    event({ kind: "answered", sdp: "their-answer" });
    await flushPromises();
    expect(calls.call.phase).toBe("connecting");
    expect(calls.media.ringback.stop).toHaveBeenCalled();
    event({ kind: "connected" });
    await flushPromises();
    expect(calls.call.phase).toBe("active");
    expect(calls.call.since).toBeGreaterThan(0);
  });

  it("answers a ringing voice call in the core", async () => {
    event({ kind: "incoming", video: false, sdp: "their-offer" });
    await calls.acceptCall();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer_native", { call: "call-1", routing: "auto" });
    expect(calls.media.getUserMedia).not.toHaveBeenCalled();
    expect(calls.call.phase).toBe("connecting");
  });

  it("mutes through the core, and follows a mute from the phone's call screen", async () => {
    await calls.startCall("ft_bob", false);
    calls.toggleMute();
    expect(calls.call.muted).toBe(true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_mute", { call: "call-1", muted: true });
    event({ kind: "muted", muted: false });
    await flushPromises();
    expect(calls.call.muted).toBe(false);
  });

  it("hangs up through the core", async () => {
    await calls.startCall("ft_bob", false);
    await calls.hangUp();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "call-1", failed: false });
    expect(calls.call.phase).toBe("ended");
  });

  it("ends as failed when the core cannot place it (no microphone)", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      command === "core_call_start_native" ? Promise.reject(new Error("the microphone is not allowed")) : Promise.resolve(answers(command)),
    );
    await calls.startCall("ft_bob", false);
    expect(calls.call).toMatchObject({ phase: "ended", outcome: "failed" });
  });

  it("tells the core the routing at start", () => {
    expect(tauri.invoke).toHaveBeenCalledWith("core_set_call_routing", { routing: "auto" });
  });

  // The core heard the call before this WebView was there (PushKit woke the app).
  it("restores a call that rang before the WebView was there and answers it from the notification", async () => {
    current = { call: "c7", contact: "ft_bob", video: false, outgoing: false, phase: "ringing", offer: "their-offer", native: false, muted: false };
    pending = "answer";
    calls.reset();
    await calls.startCalls();
    await flushPromises();
    expect(calls.call).toMatchObject({ id: "c7", contact: "ft_bob", outgoing: false, video: false });
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer_native", { call: "c7", routing: "auto" });
    expect(navigation.push).toHaveBeenCalledWith("/call/ft_bob");
  });

  it("restores a ringing video call with its offer for the WebView", async () => {
    current = { call: "c8", contact: "ft_bob", video: true, outgoing: false, phase: "ringing", offer: "their-offer", native: false, muted: false };
    calls.reset();
    await calls.startCalls();
    expect(calls.call).toMatchObject({ id: "c8", phase: "ringing", video: true });
    await calls.acceptCall();
    expect(FakePeer.last.remote).toEqual({ type: "offer", sdp: "their-offer" });
  });

  // CallKit answered on the locked iPhone; the app opens during the call.
  it("shows a call CallKit already answered", async () => {
    current = { call: "c9", contact: "ft_bob", video: false, outgoing: false, phase: "active", native: true, muted: true, connectedAt: 1234 };
    pending = "answer";
    calls.reset();
    await calls.startCalls();
    await flushPromises();
    expect(calls.call).toMatchObject({ id: "c9", phase: "active", since: 1234, muted: true });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_answer_native", expect.anything());
    expect(navigation.push).toHaveBeenCalledWith("/call/ft_bob");
    calls.toggleMute();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_mute", { call: "c9", muted: false });
  });

  it("gives up a restored native call nobody answers", async () => {
    vi.useFakeTimers();
    current = { call: "c11", contact: "ft_bob", video: false, outgoing: true, phase: "calling", native: true, muted: false };
    calls.reset();
    await calls.startCalls();
    expect(calls.call).toMatchObject({ id: "c11", phase: "calling" });
    await vi.advanceTimersByTimeAsync(calls.RING_LIMIT + 1);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "c11", failed: false });
  });

  // A WebView call whose WebView is gone has no media left: it ends honestly.
  it("ends a WebView call that outlived its WebView", async () => {
    current = { call: "c10", contact: "ft_bob", video: true, outgoing: true, phase: "active", native: false, muted: false, connectedAt: 1 };
    calls.reset();
    await calls.startCalls();
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_end", { call: "c10", failed: true });
  });
});
