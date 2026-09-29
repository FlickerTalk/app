import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  handlers: {} as Record<string, (event: { payload: unknown }) => void>,
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
// Answering from the notification must open the call screen, as the in-app button does.
// The call screen is opened once: where the app already is counts (`currentRoute`).
const navigation = vi.hoisted(() => {
  const currentRoute = { value: { path: "/tabs/chats" } };
  const push = vi.fn(async (path: string) => {
    currentRoute.value.path = path;
  });
  return { push, currentRoute };
});
vi.mock("./router", () => ({ router: { push: navigation.push, currentRoute: navigation.currentRoute } }));
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
    navigation.currentRoute.value.path = "/tabs/chats";
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
// iPhone has no WebView). The WebView only shows the call. Since 2026-09-29 video calls too
// (docs/video-nativo.md): every call on a phone is native.
describe("native calls", () => {
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
    navigation.currentRoute.value.path = "/tabs/chats";
    localStorage.clear();
    calls.reset();
    await calls.startCalls();
  });

  // Bug seen on the iPhone (2026-09-29): with the app on the screen, the call answered from
  // CallKit's banner had its voice, but the app never showed the call screen, so there was no way
  // to hang up. However it was answered, a call that connects is shown.
  it("shows the call screen when a call answered elsewhere connects", async () => {
    event({ kind: "incoming", video: false, sdp: "their-offer" });
    await flushPromises();
    event({ kind: "connected" });
    await flushPromises();
    expect(calls.call.phase).toBe("active");
    expect(navigation.push).toHaveBeenCalledWith("/call/ft_bob");
  });

  it("does not open the call screen again when it is already there", async () => {
    await calls.startCall("ft_bob", false);
    navigation.currentRoute.value.path = "/call/ft_bob";
    event({ kind: "answered", sdp: "their-answer" });
    event({ kind: "connected" });
    await flushPromises();
    expect(navigation.push).not.toHaveBeenCalled();
  });

  // The same bug: CallKit's answer only reached the WebView when the app came back to the screen,
  // and with the app already there the in-app ringing stayed up. The phone says so at once now.
  it("follows at once an answer from CallKit with the app on the screen", async () => {
    event({ kind: "incoming", video: false, sdp: "their-offer" });
    await flushPromises();
    pending = "answer";
    tauri.handlers["ft://call-action"]({ payload: null });
    await flushPromises();
    expect(calls.call.phase).toBe("connecting");
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer_native", { call: "call-1", routing: "auto" });
    expect(navigation.push).toHaveBeenCalledWith("/call/ft_bob");
  });

  // CallKit's answer with the app on the screen used to wait until the app went away and came
  // back, so a video call was never answered. It is the core's to answer now, as a voice call.
  it("answers a video call that CallKit answered with the app on the screen", async () => {
    event({ kind: "incoming", video: true, sdp: "their-offer" });
    await flushPromises();
    pending = "answer";
    tauri.handlers["ft://call-action"]({ payload: null });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer_native", { call: "call-1", routing: "auto" });
    expect(calls.media.createPeer).not.toHaveBeenCalled();
  });

  // A "decline" is for a call that rings: one left behind by an earlier call, or CallKit's end of
  // a call the core already hangs up, must never hang up the call going on.
  it("never hangs up a call going on for a decline", async () => {
    event({ kind: "incoming", video: false, sdp: "their-offer" });
    await calls.acceptCall();
    event({ kind: "connected" });
    await flushPromises();
    pending = "decline";
    await calls.applyCallNotification();
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_end", expect.anything());
    expect(calls.call.phase).toBe("active");
  });

  it("places a voice call in the core, with the routing of the settings and no WebView media", async () => {
    setCallRouting("always");
    await calls.startCall("ft_bob", false);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start_native", { contact: "ft_bob", routing: "always", video: false });
    expect(calls.media.getUserMedia).not.toHaveBeenCalled();
    expect(calls.media.createPeer).not.toHaveBeenCalled();
    expect(calls.call).toMatchObject({ id: "call-1", phase: "calling", outgoing: true, video: false });
    expect(calls.media.ringback.start).toHaveBeenCalled();
  });

  it("places a video call in the core too, with no WebView camera", async () => {
    await calls.startCall("ft_bob", true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start_native", { contact: "ft_bob", routing: "auto", video: true });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_start", expect.anything());
    expect(calls.media.getUserMedia).not.toHaveBeenCalled();
    expect(calls.call).toMatchObject({ native: true, video: true });
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

  // Speaker or receiver (2026-09-28): a voice call starts on the receiver, like a phone call; a
  // video call on the speaker, which the phone is told once the call is live.
  it("starts a voice call on the receiver and switches to the speaker when asked", async () => {
    await calls.startCall("ft_bob", false);
    expect(calls.call.speaker).toBe(false);
    calls.toggleSpeaker();
    expect(calls.call.speaker).toBe(true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_speaker", { on: true });
    calls.toggleSpeaker();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_speaker", { on: false });
  });

  it("puts a video call on the speaker once it is live", async () => {
    await calls.startCall("ft_bob", true);
    expect(calls.call.speaker).toBe(true);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_speaker", expect.anything());
    event({ kind: "answered", sdp: "their-answer" });
    event({ kind: "connected" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_speaker", { on: true });
  });

  it("leaves a voice call on the receiver when it goes live", async () => {
    await calls.startCall("ft_bob", false);
    event({ kind: "answered", sdp: "their-answer" });
    event({ kind: "connected" });
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_speaker", expect.anything());
  });

  // On the iPhone the WebView plays nothing during a native call (2026-09-28): WebKit may change
  // the app's audio session, which CallKit and the native voice own.
  it("plays no WebView ringback for a native call on the iPhone", async () => {
    const agent = vi.spyOn(navigator, "userAgent", "get").mockReturnValue("Mozilla/5.0 (iPhone; CPU iPhone OS 18_5 like Mac OS X) AppleWebKit/605.1.15");
    await calls.startCall("ft_bob", false);
    expect(calls.media.ringback.start).not.toHaveBeenCalled();
    agent.mockRestore();
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

  it("restores a ringing video call and answers it in the core", async () => {
    current = { call: "c8", contact: "ft_bob", video: true, outgoing: false, phase: "ringing", offer: "their-offer", native: false, muted: false };
    calls.reset();
    await calls.startCalls();
    expect(calls.call).toMatchObject({ id: "c8", phase: "ringing", video: true });
    await calls.acceptCall();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_answer_native", { call: "c8", routing: "auto" });
    expect(calls.media.createPeer).not.toHaveBeenCalled();
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

// Native video (2026-09-29, docs/video-nativo.md): every native call can go from voice to video
// and back at any moment. Each side owns its camera; the core says what both cameras do with a
// whole snapshot (`kind: "video"`), never loose events.
describe("native video", () => {
  let current: Record<string, unknown> | null;
  let view: Record<string, unknown>;
  let denied: boolean;

  const snapshot = (patch: Record<string, unknown> = {}) => ({
    available: true,
    camera: false,
    paused: false,
    facing: "front",
    remote: false,
    remotePaused: false,
    ...patch,
  });

  function answers(command: string, args?: Record<string, unknown>) {
    switch (command) {
      case "core_native_calls":
        return Promise.resolve(true);
      case "core_call_start_native":
        return Promise.resolve("call-1");
      case "core_current_call":
        return Promise.resolve(current);
      case "core_pending_call":
        return Promise.resolve("");
      case "core_calls":
        return Promise.resolve([]);
      case "core_call_set_video":
        if (denied && args?.on) return Promise.reject("camera_denied");
        view = { ...view, camera: Boolean(args?.on) };
        return Promise.resolve(view);
      case "core_call_switch_camera":
        view = { ...view, facing: view.facing === "front" ? "back" : "front" };
        return Promise.resolve(view);
      default:
        return Promise.resolve(undefined);
    }
  }

  const event = (payload: Record<string, unknown>) => tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "call-1", ...payload } });
  const video = (patch: Record<string, unknown> = {}) => event({ kind: "video", ...snapshot(patch) });
  const frame = () => new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
  const layouts = () => tauri.invoke.mock.calls.filter(([command]) => command === "core_call_video_layout").map(([, args]) => args);

  /** A voice call going on, on the receiver, with the video line negotiated. */
  async function live() {
    await calls.startCall("ft_bob", false);
    event({ kind: "answered", sdp: "their-answer" });
    event({ kind: "connected" });
    video();
    await flushPromises();
  }

  beforeEach(async () => {
    // A test above may have left fake timers, and with them a frame that never comes.
    vi.useRealTimers();
    current = null;
    view = snapshot();
    denied = false;
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation(answers);
    calls.media.getUserMedia = vi.fn();
    calls.media.createPeer = vi.fn();
    calls.media.ringback = { start: vi.fn(), stop: vi.fn() };
    navigation.currentRoute.value.path = "/call/ft_bob";
    localStorage.clear();
    calls.reset();
    await calls.startCalls();
  });

  /** The core's view of call-1, as `core_current_call` says it. */
  const going = (phase: string, patch: Record<string, unknown> = {}) => {
    current = { call: "call-1", contact: "ft_bob", video: snapshot(patch), outgoing: true, phase, native: true, muted: false };
  };

  // Found by QA on the emulators (2026-09-29): the camera was marked on in advance, and when it
  // failed to start nothing ever corrected it. The camera is what the core says, not a guess.
  it("shows the camera as the core has it once a video call is placed", async () => {
    going("calling", { available: false, camera: true });
    await calls.startCall("ft_bob", true);
    expect(calls.call.view).toMatchObject({ available: false, camera: true, remote: false });
    event({ kind: "video", ...snapshot({ camera: true, paused: true }) });
    await flushPromises();
    expect(calls.call.view).toMatchObject({ available: true, camera: true, paused: true });
  });

  it("guesses nothing about the camera when the core does not say", async () => {
    await calls.startCall("ft_bob", true);
    expect(calls.call.view.camera).toBe(false);
  });

  /** How many times the WebView asked the core for its call. */
  const reads = () => tauri.invoke.mock.calls.filter(([command]) => command === "core_current_call").length;

  // The core readies the video after it says the call connected. When the camera it wanted cannot
  // start there (an encoder that cannot be configured, on the emulator) it says so with its own
  // event (2026-09-29): the WebView follows the events, it does not ask the core in a loop.
  it("learns from the core that the camera did not start as the call connected", async () => {
    going("calling", { available: false, camera: true });
    await calls.startCall("ft_bob", true);
    event({ kind: "answered", sdp: "" });
    event({ kind: "connected" });
    await flushPromises();
    expect(calls.call.view.camera).toBe(true);
    event({ kind: "camera_failed" });
    await flushPromises();
    expect(calls.call).toMatchObject({ phase: "active", cameraFailed: true });
    expect(calls.call.view.camera).toBe(false);
    video({ camera: false });
    await flushPromises();
    expect(calls.call).toMatchObject({ phase: "active", cameraFailed: true });
    expect(calls.call.view).toMatchObject({ available: true, camera: false });
  });

  it("learns that a voice call has a video line from the core's event as it connects", async () => {
    going("calling", { available: false });
    await calls.startCall("ft_bob", false);
    event({ kind: "answered", sdp: "" });
    event({ kind: "connected" });
    await flushPromises();
    expect(calls.call.view.available).toBe(false);
    video();
    await flushPromises();
    expect(calls.call.view.available).toBe(true);
    expect(calls.call.cameraFailed).toBe(false);
  });

  // The owner's rule (2026-09-29): no workarounds. Placing a call reads the core once; after that
  // only its events move the screen, however long the video takes.
  it("does not ask the core again while the video of a connected call gets ready", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    going("calling", { available: false, camera: true });
    await calls.startCall("ft_bob", true);
    const placed = reads();
    event({ kind: "answered", sdp: "" });
    going("active", { available: true, camera: false });
    event({ kind: "connected" });
    await vi.advanceTimersByTimeAsync(10_000);
    expect(reads()).toBe(placed);
    expect(calls.call).toMatchObject({ phase: "active", cameraFailed: false });
    expect(calls.call.view).toMatchObject({ available: false, camera: true });
  });

  it("starts a voice call with the camera off", async () => {
    await calls.startCall("ft_bob", false);
    expect(calls.call.view).toMatchObject({ available: false, camera: false, remote: false });
  });

  it("follows the snapshots of both cameras", async () => {
    await live();
    video({ remote: true });
    await flushPromises();
    expect(calls.call.view).toMatchObject({ available: true, camera: false, remote: true, remotePaused: false });
    // Their phone locked or went to the background: their camera is held.
    video({ remote: true, remotePaused: true });
    await flushPromises();
    expect(calls.call.view.remotePaused).toBe(true);
    video({ remote: false });
    await flushPromises();
    expect(calls.call.view.remote).toBe(false);
  });

  it("ignores the video of another call", async () => {
    await live();
    tauri.handlers["ft://call"]({ payload: { contact: "ft_carol", call: "other", kind: "video", ...snapshot({ remote: true }) } });
    await flushPromises();
    expect(calls.call.view.remote).toBe(false);
  });

  it("turns my camera on and off through the core", async () => {
    await live();
    await calls.toggleCamera();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_set_video", { call: "call-1", on: true });
    expect(calls.call.view.camera).toBe(true);
    await calls.toggleCamera();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_set_video", { call: "call-1", on: false });
    expect(calls.call.view.camera).toBe(false);
    expect(calls.media.getUserMedia).not.toHaveBeenCalled();
  });

  it("switches between the front and the back camera", async () => {
    await live();
    await calls.toggleCamera();
    await calls.switchCamera();
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_switch_camera", { call: "call-1" });
    expect(calls.call.view.facing).toBe("back");
  });

  // §30: the camera is asked for when it is turned on. Denied, the call goes on as a voice call
  // and the screen says why.
  it("says so when the camera is not allowed, and keeps the call", async () => {
    await live();
    denied = true;
    await calls.toggleCamera();
    expect(calls.call).toMatchObject({ phase: "active", cameraDenied: true });
    expect(calls.call.view.camera).toBe(false);
    denied = false;
    await calls.toggleCamera();
    expect(calls.call).toMatchObject({ cameraDenied: false });
    expect(calls.call.view.camera).toBe(true);
  });

  // Found by QA on the emulators (2026-09-29): any other camera error said nothing at all.
  it("says so when the camera cannot start for another reason, and keeps the call", async () => {
    await live();
    tauri.invoke.mockImplementation((command: string, args?: Record<string, unknown>) =>
      command === "core_call_set_video" && args?.on ? Promise.reject("the camera cannot start: configure failed") : answers(command, args),
    );
    await calls.toggleCamera();
    expect(calls.call).toMatchObject({ phase: "active", cameraFailed: true, cameraDenied: false });
    expect(calls.call.view.camera).toBe(false);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_end", expect.anything());
    tauri.invoke.mockImplementation(answers);
    await calls.toggleCamera();
    expect(calls.call.cameraFailed).toBe(false);
    expect(calls.call.view.camera).toBe(true);
  });

  it("places a video call as a voice call when the camera is not allowed", async () => {
    tauri.invoke.mockImplementation((command: string, args?: Record<string, unknown>) =>
      command === "core_call_start_native" && args?.video ? Promise.reject("camera_denied") : answers(command, args),
    );
    await calls.startCall("ft_bob", true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_start_native", { contact: "ft_bob", routing: "auto", video: false });
    expect(calls.call).toMatchObject({ id: "call-1", phase: "calling", cameraDenied: true });
    expect(calls.call.view.camera).toBe(false);
  });

  it("answers a video call as a voice call when the camera is not allowed", async () => {
    tauri.invoke.mockImplementation((command: string, args?: Record<string, unknown>) =>
      command === "core_call_answer_native" ? Promise.reject("camera_denied") : answers(command, args),
    );
    event({ kind: "incoming", video: true, sdp: "their-offer" });
    await calls.acceptCall();
    expect(calls.call).toMatchObject({ phase: "connecting", cameraDenied: true });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_end", expect.anything());
  });

  // With pictures on the screen nobody holds the phone to the ear.
  it("moves the voice to the speaker when video comes in", async () => {
    await live();
    expect(calls.call.speaker).toBe(false);
    video({ remote: true });
    await flushPromises();
    expect(calls.call.speaker).toBe(true);
    expect(tauri.invoke).toHaveBeenCalledWith("core_call_speaker", { on: true });
    tauri.invoke.mockClear();
    video({ remote: true, camera: true });
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_speaker", expect.anything());
  });

  it("measures a video's place in CSS pixels", () => {
    const slot = document.createElement("div");
    slot.getBoundingClientRect = () => ({ x: 278, y: 594, width: 96, height: 140 }) as DOMRect;
    expect(calls.rectOf(slot)).toEqual({ x: 278, y: 594, width: 96, height: 140 });
    expect(calls.rectOf(null)).toBeNull();
  });

  it("tells the core where the video goes, at most once a frame and only when it changes", async () => {
    await live();
    const layout = { remote: { x: 0, y: 0, width: 390, height: 844 }, local: null, mirrorLocal: true, localRadius: 16 };
    calls.layoutVideo(() => layout);
    calls.layoutVideo(() => layout);
    expect(layouts()).toEqual([]);
    await frame();
    expect(layouts()).toEqual([{ layout }]);
    calls.layoutVideo(() => layout);
    await frame();
    expect(layouts()).toHaveLength(1);
    const moved = { ...layout, local: { x: 20, y: 600, width: 96, height: 140 } };
    calls.layoutVideo(() => moved);
    await frame();
    expect(layouts()).toEqual([{ layout }, { layout: moved }]);
  });

  // The native views may come up after the layout was sent: every snapshot sends it again.
  it("sends the layout again after each video snapshot", async () => {
    await live();
    const layout = { remote: null, local: null, mirrorLocal: true, localRadius: 16 };
    calls.layoutVideo(() => layout);
    await frame();
    video({ remote: true });
    await flushPromises();
    calls.layoutVideo(() => layout);
    await frame();
    expect(layouts()).toHaveLength(2);
  });

  it("hides the video when the call screen goes", async () => {
    await live();
    calls.layoutVideo(() => ({ remote: null, local: null, mirrorLocal: true, localRadius: 16 }));
    calls.hideVideo();
    await frame();
    expect(layouts()).toEqual([{ layout: null }]);
  });

  it("restores a call whose video is not ready yet with one read, then follows the events", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    current = { call: "c9", contact: "ft_bob", video: snapshot({ available: false }), outgoing: false, phase: "active", native: true, muted: false, connectedAt: 1 };
    calls.reset();
    const before = reads();
    await calls.startCalls();
    expect(calls.call.view.available).toBe(false);
    current = { ...current, video: snapshot() };
    await vi.advanceTimersByTimeAsync(10_000);
    expect(reads() - before).toBe(1);
    expect(calls.call.view.available).toBe(false);
    await tauri.handlers["ft://call"]({ payload: { contact: "ft_bob", call: "c9", kind: "video", ...snapshot() } });
    expect(calls.call.view.available).toBe(true);
  });

  it("restores a native video call with both cameras", async () => {
    current = {
      call: "c9",
      contact: "ft_bob",
      video: snapshot({ camera: true, remote: true }),
      outgoing: false,
      phase: "active",
      native: true,
      muted: false,
      connectedAt: 1,
    };
    calls.reset();
    await calls.startCalls();
    expect(calls.call).toMatchObject({ id: "c9", native: true, video: true, speaker: true });
    expect(calls.call.view).toMatchObject({ available: true, camera: true, remote: true });
  });
});

describe("video on the desktop", () => {
  beforeEach(async () => {
    vi.useRealTimers();
    tauri.invoke.mockReset();
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_call_ice" ? servers : command === "core_call_start" ? "call-1" : command === "core_calls" ? [] : undefined),
    );
    calls.media.getUserMedia = vi.fn(async (constraints: MediaStreamConstraints) => new FakeStream(Boolean(constraints.video)) as unknown as MediaStream);
    calls.media.createPeer = (config: RTCConfiguration) => new FakePeer(config) as unknown as RTCPeerConnection;
    calls.media.ringback = { start: vi.fn(), stop: vi.fn() };
    calls.reset();
    await calls.startCalls();
  });

  it("keeps the WebView's media and sends no native layout", async () => {
    await calls.startCall("ft_bob", true);
    expect(calls.call.native).toBe(false);
    calls.layoutVideo(() => ({ remote: null, local: null, mirrorLocal: true, localRadius: 16 }));
    calls.hideVideo();
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_call_video_layout", expect.anything());
  });
});
