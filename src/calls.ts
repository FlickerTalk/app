/**
 * Voice and video calls (Plan §66, §106 M6), one to one. The core carries the offer, the answer
 * and the end of each call, encrypted and directly, and keeps the history. Descriptions are sent
 * whole, with their ICE candidates (no trickle).
 *
 * Where the media runs (2026-09-28): on the phones, a call's media runs in the Rust core
 * (`core_call_*_native`), because CallKit answers on a locked iPhone with no WebView at all; this
 * file only shows it, from the core's events. Since 2026-09-29 that includes video
 * (docs/video-nativo.md): the native views sit under the WebView, which says where they go
 * (`layoutVideo`), and either side turns its own camera on or off at any moment. The desktop keeps
 * the WebView's WebRTC (`getUserMedia` + `RTCPeerConnection`), with no switching.
 */
import { markRaw, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { storedCallRouting, syncCallRouting } from "./preferences";
import { router } from "./router";

export type CallPhase = "idle" | "calling" | "ringing" | "connecting" | "active" | "ended";
export type CallOutcome = "answered" | "missed" | "declined" | "busy" | "cancelled" | "unreachable" | "failed";

/** Both cameras of a native call, as the core last said (`kind: "video"`). */
export interface CallVideo {
  /** The call has a video line both ways: the camera can be turned on. */
  available: boolean;
  /** My camera is on. */
  camera: boolean;
  /** My camera is on but held: the app is not on the screen, or the phone is locked. */
  paused: boolean;
  facing: "front" | "back";
  /** Their camera is on. */
  remote: boolean;
  /** Their camera is on but held. */
  remotePaused: boolean;
}

/** Where the native pictures go, in CSS pixels (the bridge's `VideoLayout`). */
export interface VideoRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface VideoLayout {
  remote: VideoRect | null;
  local: VideoRect | null;
  /** My picture as in a mirror (the front camera). */
  mirrorLocal: boolean;
  /** The corners of my picture, in CSS pixels. */
  localRadius: number;
}

export interface CallState {
  id: string;
  contact: string;
  /** A video call: placed or offered with the camera on. */
  video: boolean;
  outgoing: boolean;
  phase: CallPhase;
  outcome: CallOutcome | null;
  /** When the media connected (ms), for the call's clock. */
  since: number;
  muted: boolean;
  /** The voice on the speaker; otherwise on the receiver, like a phone call. */
  speaker: boolean;
  /** The WebView's camera is off (the desktop). */
  cameraOff: boolean;
  local: MediaStream | null;
  remote: MediaStream | null;
  /** The core carries the media (the phones). */
  native: boolean;
  /** The native call's cameras. */
  view: CallVideo;
  /** The camera was asked for and not allowed: the call goes on as voice. */
  cameraDenied: boolean;
  /** The camera was wanted and could not start (an encoder that cannot be set up, say): voice. */
  cameraFailed: boolean;
}

export interface CallEntry {
  id: string;
  contact: string;
  name: string;
  outgoing: boolean;
  video: boolean;
  startedAt: number;
  seconds: number;
  outcome: CallOutcome | null;
}

interface CallEvent extends Partial<CallVideo> {
  contact: string;
  call: string;
  kind: "incoming" | "answered" | "connected" | "muted" | "ended" | "video";
  video?: boolean;
  sdp?: string;
  outcome?: CallOutcome;
  muted?: boolean;
}

/** The call the core has going on, for a WebView that comes up after it started. */
interface CurrentCall {
  call: string;
  contact: string;
  /** Whether it is a video call or, from the native video on, both cameras. */
  video: boolean | CallVideo;
  outgoing: boolean;
  phase: "calling" | "ringing" | "connecting" | "active";
  offer?: string;
  native: boolean;
  muted: boolean;
  connectedAt?: number;
}

export const CALL_EVENT = "ft://call";
/**
 * The phone's own call screen did something (2026-09-29): CallKit answered, with the app maybe
 * on the screen. What it was waits in `core_pending_call`, as for a notification.
 */
export const CALL_ACTION_EVENT = "ft://call-action";
/** The caller gives up after this long without an answer. */
export const RING_LIMIT = 45_000;
/** How often, and how many times, the core is asked for the video it readies as the call connects. */
const SETTLE_STEP = 250;
const SETTLE_TRIES = 20;
/** The longest wait for ICE candidates before the description goes anyway. */
const GATHER_LIMIT = 3_000;

interface Tone {
  start(): void;
  stop(): void;
}

/** The ringback tone the caller hears (425 Hz, 1 s on and 4 s off), made with Web Audio. */
function webRingback(): Tone {
  let context: AudioContext | null = null;
  return {
    start() {
      if (context) return;
      context = new AudioContext();
      const gain = context.createGain();
      gain.gain.value = 0;
      gain.connect(context.destination);
      const tone = context.createOscillator();
      tone.frequency.value = 425;
      tone.connect(gain);
      const now = context.currentTime;
      for (let second = 0; second < RING_LIMIT / 1000; second += 5) {
        gain.gain.setValueAtTime(0.12, now + second);
        gain.gain.setValueAtTime(0, now + second + 1);
      }
      tone.start();
    },
    stop() {
      void context?.close();
      context = null;
    },
  };
}

/** The browser's media and WebRTC, replaceable in tests. */
export const media = {
  getUserMedia: (constraints: MediaStreamConstraints) => navigator.mediaDevices.getUserMedia(constraints),
  createPeer: (config: RTCConfiguration) => new RTCPeerConnection(config),
  ringback: webRingback() as Tone,
};

const noVideo = (): CallVideo => ({
  available: false,
  camera: false,
  paused: false,
  facing: "front",
  remote: false,
  remotePaused: false,
});

const idle = (): CallState => ({
  id: "",
  contact: "",
  video: false,
  outgoing: false,
  phase: "idle",
  outcome: null,
  since: 0,
  muted: false,
  speaker: false,
  cameraOff: false,
  local: null,
  remote: null,
  native: false,
  view: noVideo(),
  cameraDenied: false,
  cameraFailed: false,
});

export const call = reactive<CallState>(idle());
export const history = reactive({ calls: [] as CallEntry[] });

let peer: RTCPeerConnection | null = null;
let offer = "";
let ringTimer: ReturnType<typeof setTimeout> | undefined;
let settleTimer: ReturnType<typeof setTimeout> | undefined;
let listening = false;
/** Whether this phone runs calls in the core (iOS and Android): voice and video alike. */
let nativeCalls = false;
/** Whether the current call's media is the core's. */
let nativeCall = false;

function setNative(native: boolean) {
  nativeCall = native;
  call.native = native;
}

/** What the core answers when the camera is not allowed (`core_call_set_video`). */
const cameraDenied = (error: unknown) => String(error).includes("camera_denied");

/**
 * Whether the WebView may play the ringback (2026-09-28). Not during a native call on the
 * iPhone: WebKit may change the app's audio session, which CallKit and the native voice own.
 */
const webViewMayPlay = () => !(nativeCall && /iPhone|iPad/.test(navigator.userAgent));

/** The call goes live: a video call's voice goes to the speaker (a voice call stays on the receiver). */
function goLive() {
  call.phase = "active";
  call.since = Date.now();
  if (call.speaker) void invoke("core_call_speaker", { on: true }).catch(() => undefined);
}

function busy(): boolean {
  return call.phase !== "idle" && call.phase !== "ended";
}

const isTurn = (server: RTCIceServer) => [server.urls].flat().some((url) => url.startsWith("turn"));

/** STUN and TURN from the router, filtered by the routing chosen in Settings (§17). */
async function iceConfig(): Promise<RTCConfiguration> {
  const servers = await invoke<RTCIceServer[]>("core_call_ice");
  switch (storedCallRouting()) {
    case "direct":
      return { iceServers: servers.filter((server) => !isTurn(server)) };
    case "always":
      return { iceServers: servers.filter(isTurn), iceTransportPolicy: "relay" };
    default:
      return { iceServers: servers };
  }
}

/** Our description once its candidates are in, or when the limit passes. */
async function gathered(pc: RTCPeerConnection): Promise<string> {
  if (pc.iceGatheringState !== "complete") {
    await new Promise<void>((resolve) => {
      const done = setTimeout(resolve, GATHER_LIMIT);
      pc.onicegatheringstatechange = () => {
        if (pc.iceGatheringState === "complete") {
          clearTimeout(done);
          resolve();
        }
      };
    });
  }
  return pc.localDescription?.sdp ?? "";
}

async function preparePeer(video: boolean): Promise<RTCPeerConnection> {
  const local = await media.getUserMedia({ audio: true, video: video ? { facingMode: "user" } : false });
  call.local = markRaw(local);
  const pc = media.createPeer(await iceConfig());
  peer = pc;
  for (const track of local.getTracks()) {
    pc.addTrack(track, local);
  }
  pc.ontrack = (event) => {
    call.remote = markRaw(event.streams[0]);
  };
  pc.onconnectionstatechange = () => {
    if (pc.connectionState === "connected" && call.phase !== "active") {
      goLive();
    } else if (pc.connectionState === "failed") {
      void fail();
    }
  };
  return pc;
}

/** Lets go of the camera, the microphone and the connection. */
function release() {
  cancelAnimationFrame(layoutFrame);
  layoutFrame = 0;
  clearTimeout(ringTimer);
  clearTimeout(settleTimer);
  media.ringback.stop();
  call.local?.getTracks().forEach((track) => track.stop());
  peer?.close();
  peer = null;
}

function finish() {
  release();
  call.phase = "ended";
}

/** Back to no call at all. */
export function reset() {
  release();
  offer = "";
  sentLayout = undefined;
  Object.assign(call, idle());
  setNative(false);
}

/** Calls the contact. If it cannot start (no camera or microphone, say), it ends as failed. */
export async function startCall(contact: string, video: boolean): Promise<void> {
  if (busy()) throw new Error("already in a call");
  Object.assign(call, idle(), { contact, video, outgoing: true, phase: "calling", speaker: video });
  setNative(nativeCalls);
  try {
    if (nativeCall) {
      // The core turns the camera on once the call connects; until then the camera is its wish.
      call.id = await startNativeCall(contact, video);
      await followCore();
    } else {
      await startWebCall(contact, video);
    }
  } catch {
    await fail();
    return;
  }
  if (webViewMayPlay()) media.ringback.start();
  giveUpUnanswered();
}

/** A native call; without the camera allowed, a video call is placed as a voice call. */
async function startNativeCall(contact: string, video: boolean): Promise<string> {
  const routing = storedCallRouting();
  try {
    return await invoke<string>("core_call_start_native", { contact, routing, video });
  } catch (error) {
    if (!video || !cameraDenied(error)) throw error;
    call.cameraDenied = true;
    return await invoke<string>("core_call_start_native", { contact, routing, video: false });
  }
}

/** The caller gives up when nobody answers. */
function giveUpUnanswered() {
  clearTimeout(ringTimer);
  ringTimer = setTimeout(() => {
    if (call.phase === "calling") void hangUp();
  }, RING_LIMIT);
}

/** The WebView's own media: camera and microphone, and its RTCPeerConnection's offer. */
async function startWebCall(contact: string, video: boolean) {
  const pc = await preparePeer(video);
  await pc.setLocalDescription(await pc.createOffer());
  call.id = await invoke<string>("core_call_start", { contact, video, sdp: await gathered(pc) });
}

export async function acceptCall(): Promise<void> {
  if (call.phase !== "ringing") return;
  call.phase = "connecting";
  setNative(nativeCalls);
  try {
    if (nativeCall) {
      await answerNativeCall();
      return;
    }
    const pc = await preparePeer(call.video);
    await pc.setRemoteDescription({ type: "offer", sdp: offer });
    await pc.setLocalDescription(await pc.createAnswer());
    await invoke("core_call_answer", { call: call.id, sdp: await gathered(pc) });
  } catch {
    await fail();
  }
}

/** Answers in the core. A video call with the camera not allowed is answered all the same, as voice. */
async function answerNativeCall() {
  try {
    await invoke("core_call_answer_native", { call: call.id, routing: storedCallRouting() });
  } catch (error) {
    if (!call.video || !cameraDenied(error)) throw error;
    call.cameraDenied = true;
  }
}

/**
 * The call screen, unless the app is on it already: the only place where a call is seen and hung
 * up (2026-09-29). Whichever way the call was answered (the app, CallKit's banner or lock screen,
 * the notification), it must end up there.
 */
async function showCall(): Promise<void> {
  const screen = `/call/${call.contact}`;
  if (call.contact && router.currentRoute.value.path !== screen) await router.push(screen);
}

/**
 * What the user pressed on the phone's own call screen or notification (§66). The app asks for
 * it when it opens or comes back, because a call may have been answered there while the WebView
 * was not even running, and when the phone says so (`CALL_ACTION_EVENT`). A decline is only ever
 * for the call that rings: never a hang-up of a call going on.
 */
export async function applyCallNotification(): Promise<void> {
  const action = (await invoke<string>("core_pending_call").catch(() => "")) ?? "";
  if (call.phase !== "ringing") return;
  if (action === "answer") {
    // Like the in-app button: the call screen is where the call is seen and hung up.
    const accepting = acceptCall();
    await showCall();
    await accepting;
  } else if (action === "decline") await hangUp();
}

/** Hangs up, declines or gives up, whichever it is by now. */
export async function hangUp(): Promise<void> {
  const id = call.id;
  finish();
  if (id) await invoke("core_call_end", { call: id, failed: false });
}

async function fail() {
  const id = call.id;
  finish();
  call.outcome = "failed";
  if (id) await invoke("core_call_end", { call: id, failed: true });
}

export function toggleMute() {
  call.muted = !call.muted;
  if (nativeCall) void invoke("core_call_mute", { call: call.id, muted: call.muted }).catch(() => undefined);
  else call.local?.getAudioTracks().forEach((track) => (track.enabled = !call.muted));
}

/** Speaker or receiver: the phone moves the call's voice (2026-09-28). */
export function toggleSpeaker() {
  call.speaker = !call.speaker;
  void invoke("core_call_speaker", { on: call.speaker }).catch(() => undefined);
}

/**
 * My camera on or off. On the phones it is the voice/video switch, at any moment: the core asks
 * for the camera when it is turned on (§30) and tells the other side. On the desktop it only
 * blanks the WebView's camera of a video call.
 */
export async function toggleCamera(): Promise<void> {
  if (!nativeCall) {
    call.cameraOff = !call.cameraOff;
    call.local?.getVideoTracks().forEach((track) => (track.enabled = !call.cameraOff));
    return;
  }
  const on = !call.view.camera;
  try {
    applyVideo(await invoke<CallVideo>("core_call_set_video", { call: call.id, on }));
    call.cameraDenied = false;
    if (on) call.cameraFailed = false;
  } catch (error) {
    if (on && cameraDenied(error)) call.cameraDenied = true;
    else if (on) call.cameraFailed = true;
    // Whatever went wrong, the screen shows the camera as the core has it.
    await followCore();
  }
}

/** The front or the back camera (native calls). */
export async function switchCamera(): Promise<void> {
  if (!nativeCall) return;
  const view = await invoke<CallVideo>("core_call_switch_camera", { call: call.id }).catch(() => null);
  if (view) applyVideo(view);
}

/** Whether a call shows pictures: either camera on. */
const anyVideo = (view: CallVideo) => view.camera || view.remote;

/**
 * The core's snapshot of both cameras. When pictures come into a call the voice goes to the
 * speaker, if it was on the receiver: nobody holds the phone to the ear to watch. The native
 * views may have come up only now, so the layout goes again.
 */
function applyVideo(update: Partial<CallVideo>) {
  const before = anyVideo(call.view);
  const view: CallVideo = { ...call.view };
  for (const key of Object.keys(view) as (keyof CallVideo)[]) {
    if (update[key] !== undefined) (view as unknown as Record<string, unknown>)[key] = update[key];
  }
  call.view = view;
  sentLayout = undefined;
  if (!before && anyVideo(view) && !call.speaker) {
    call.speaker = true;
    void invoke("core_call_speaker", { on: true }).catch(() => undefined);
  }
}

/**
 * The cameras as the core has them now (`core_current_call`), for what no event says. Returns
 * them, or `null` when the core has no video for this call to say.
 */
async function followCore(): Promise<CallVideo | null> {
  const id = call.id;
  const current = await invoke<CurrentCall | null>("core_current_call").catch(() => null);
  if (!current || current.call !== id || call.id !== id || typeof current.video !== "object" || !current.video) return null;
  applyVideo(current.video);
  return call.view;
}

/**
 * Found by QA on the emulators (2026-09-29): as the call connects the core readies its video and
 * turns a video call's camera on, but it says nothing when the video line comes up or when the
 * camera fails to start. So the core is asked until the video is ready (or a few seconds pass);
 * a camera it wanted and could not run is said on the screen, and the call goes on as voice.
 */
async function settleVideo(wanted = false, tries = SETTLE_TRIES): Promise<void> {
  clearTimeout(settleTimer);
  const view = await followCore();
  if (!view || !busy()) return;
  // Before the video is ready the core's camera is the wish.
  const want = wanted || view.camera;
  if (!view.available && tries > 1) {
    settleTimer = setTimeout(() => void settleVideo(want, tries - 1), SETTLE_STEP);
    return;
  }
  if (want && !call.cameraDenied && (!view.available || !view.camera)) call.cameraFailed = true;
}

/** A video's place on the screen, in CSS pixels. */
export function rectOf(element: Element | null | undefined): VideoRect | null {
  if (!element) return null;
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
}

let layoutFrame = 0;
let measureLayout: (() => VideoLayout | null) | null = null;
/** What the core was last told, as JSON; `undefined` when it must be told again. */
let sentLayout: string | undefined;

function sendLayout(layout: VideoLayout | null) {
  const said = JSON.stringify(layout);
  if (said === sentLayout) return;
  sentLayout = said;
  void invoke("core_call_video_layout", { layout }).catch(() => undefined);
}

/**
 * Tells the core where the native pictures go (§3 of docs/video-nativo.md): at most once an
 * animation frame, measured then, and only when it changed. For the call screen: on showing,
 * resizing, turning and dragging.
 */
export function layoutVideo(measure: () => VideoLayout | null): void {
  if (!nativeCall) return;
  measureLayout = measure;
  if (layoutFrame) return;
  layoutFrame = requestAnimationFrame(() => {
    layoutFrame = 0;
    const layout = measureLayout?.();
    if (layout !== undefined && nativeCall) sendLayout(layout);
  });
}

/** The call screen is gone: the native views hide and my camera is held. */
export function hideVideo(): void {
  cancelAnimationFrame(layoutFrame);
  layoutFrame = 0;
  measureLayout = null;
  if (nativeCall) sendLayout(null);
}

export async function loadHistory(): Promise<void> {
  history.calls = await invoke<CallEntry[]>("core_calls");
}

async function onEvent(event: CallEvent) {
  if (event.kind === "incoming" && !busy()) {
    const video = Boolean(event.video);
    Object.assign(call, idle(), { id: event.call, contact: event.contact, video, phase: "ringing", speaker: video });
    offer = event.sdp ?? "";
  } else if (event.call === call.id && event.kind === "answered" && (peer || nativeCall)) {
    clearTimeout(ringTimer);
    media.ringback.stop();
    call.phase = "connecting";
    // A native call took the answer in the core already.
    await peer?.setRemoteDescription({ type: "answer", sdp: event.sdp ?? "" });
    await showCall();
  } else if (event.call === call.id && event.kind === "connected" && call.phase !== "active") {
    // Answered by CallKit, say, while the app only showed it ringing: the call is on now.
    goLive();
    if (nativeCall) void settleVideo();
    await showCall();
  } else if (event.call === call.id && event.kind === "video" && nativeCall) {
    applyVideo(event);
    if (call.view.camera) call.cameraFailed = false;
  } else if (event.call === call.id && event.kind === "muted") {
    call.muted = Boolean(event.muted);
  } else if (event.call === call.id && event.kind === "ended") {
    if (call.phase !== "ended") finish();
    call.outcome = event.outcome ?? null;
  }
  if (event.kind === "incoming" || event.kind === "ended") await loadHistory();
}

/**
 * The call the core has going on, shown again (2026-09-28): it may have rung, or CallKit may
 * have answered it, before this WebView listened. A call the WebView carried and lost with it has
 * no media left and ends as failed.
 */
async function restoreCall(): Promise<void> {
  const current = await invoke<CurrentCall | null>("core_current_call").catch(() => null);
  if (!current || busy()) return;
  const live = current.phase === "connecting" || current.phase === "active";
  if (!current.native && current.phase !== "ringing") {
    await invoke("core_call_end", { call: current.call, failed: true }).catch(() => undefined);
    return;
  }
  // A core from before the native video says only whether it is a video call.
  const view = typeof current.video === "object" && current.video ? { ...noVideo(), ...current.video } : noVideo();
  const video = typeof current.video === "object" ? anyVideo(view) : Boolean(current.video);
  Object.assign(call, idle(), {
    id: current.call,
    contact: current.contact,
    video,
    outgoing: current.outgoing,
    phase: current.phase,
    muted: current.muted,
    speaker: video,
    since: current.connectedAt ?? 0,
    view,
  });
  offer = current.offer ?? "";
  setNative(current.native);
  if (current.phase === "calling") giveUpUnanswered();
  // Its video may still be getting ready, with no event to say when it is.
  if (live && current.native) void settleVideo();
  // Without the call screen there is no way to hang up.
  if (live) await showCall();
}

/** Listens to the core's call events and picks up the call it may already have; at start. */
export async function startCalls(): Promise<void> {
  nativeCalls = Boolean(await invoke<boolean>("core_native_calls").catch(() => false));
  await syncCallRouting();
  if (!listening) {
    listening = true;
    await listen<CallEvent>(CALL_EVENT, ({ payload }) => void onEvent(payload));
    // CallKit answered with the app on the screen: no visibility change tells the WebView.
    await listen(CALL_ACTION_EVENT, () => void applyCallNotification());
    // The user may have answered from the notification before this WebView was even there (§66).
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") void applyCallNotification();
    });
  }
  await restoreCall();
  await applyCallNotification();
}
