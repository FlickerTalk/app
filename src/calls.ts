/**
 * Voice and video calls (Plan §66, §106 M6), one to one. The core carries the offer, the answer
 * and the end of each call, encrypted and directly, and keeps the history. Descriptions are sent
 * whole, with their ICE candidates (no trickle).
 *
 * Where the media runs (2026-09-28): on the phones, a voice call's media runs in the Rust core
 * (`core_call_*_native`), because CallKit answers on a locked iPhone with no WebView at all; this
 * file only shows it, from the core's events. Video calls and the desktop keep the WebView's
 * WebRTC (`getUserMedia` + `RTCPeerConnection`).
 */
import { markRaw, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { storedCallRouting, syncCallRouting } from "./preferences";
import { router } from "./router";

export type CallPhase = "idle" | "calling" | "ringing" | "connecting" | "active" | "ended";
export type CallOutcome = "answered" | "missed" | "declined" | "busy" | "cancelled" | "unreachable" | "failed";

export interface CallState {
  id: string;
  contact: string;
  video: boolean;
  outgoing: boolean;
  phase: CallPhase;
  outcome: CallOutcome | null;
  /** When the media connected (ms), for the call's clock. */
  since: number;
  muted: boolean;
  cameraOff: boolean;
  local: MediaStream | null;
  remote: MediaStream | null;
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

interface CallEvent {
  contact: string;
  call: string;
  kind: "incoming" | "answered" | "connected" | "muted" | "ended";
  video?: boolean;
  sdp?: string;
  outcome?: CallOutcome;
  muted?: boolean;
}

/** The call the core has going on, for a WebView that comes up after it started. */
interface CurrentCall {
  call: string;
  contact: string;
  video: boolean;
  outgoing: boolean;
  phase: "calling" | "ringing" | "connecting" | "active";
  offer?: string;
  native: boolean;
  muted: boolean;
  connectedAt?: number;
}

export const CALL_EVENT = "ft://call";
/** The caller gives up after this long without an answer. */
export const RING_LIMIT = 45_000;
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

const idle = (): CallState => ({
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
});

export const call = reactive<CallState>(idle());
export const history = reactive({ calls: [] as CallEntry[] });

let peer: RTCPeerConnection | null = null;
let offer = "";
let ringTimer: ReturnType<typeof setTimeout> | undefined;
let listening = false;
/** Whether this phone runs voice calls in the core (iOS and Android). */
let nativeVoice = false;
/** Whether the current call's media is the core's. */
let nativeCall = false;

const runsNatively = (video: boolean) => nativeVoice && !video;

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
      call.phase = "active";
      call.since = Date.now();
    } else if (pc.connectionState === "failed") {
      void fail();
    }
  };
  return pc;
}

/** Lets go of the camera, the microphone and the connection. */
function release() {
  clearTimeout(ringTimer);
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
  nativeCall = false;
  Object.assign(call, idle());
}

/** Calls the contact. If it cannot start (no camera or microphone, say), it ends as failed. */
export async function startCall(contact: string, video: boolean): Promise<void> {
  if (busy()) throw new Error("already in a call");
  Object.assign(call, idle(), { contact, video, outgoing: true, phase: "calling" });
  nativeCall = runsNatively(video);
  try {
    if (nativeCall) {
      call.id = await invoke<string>("core_call_start_native", { contact, routing: storedCallRouting() });
    } else {
      await startWebCall(contact, video);
    }
  } catch {
    await fail();
    return;
  }
  media.ringback.start();
  giveUpUnanswered();
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
  nativeCall = runsNatively(call.video);
  try {
    if (nativeCall) {
      await invoke("core_call_answer_native", { call: call.id, routing: storedCallRouting() });
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

/**
 * What the user pressed on the call notification of the phone (§66). The app asks for it when it
 * opens or comes back, because a call may have been answered from the notification while the
 * WebView was not even running.
 */
export async function applyCallNotification(): Promise<void> {
  const action = (await invoke<string>("core_pending_call").catch(() => "")) ?? "";
  if (action === "answer" && call.phase === "ringing") {
    // Like the in-app button: the call screen is where the call is seen and hung up.
    const accepting = acceptCall();
    await router.push(`/call/${call.contact}`);
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

export function toggleCamera() {
  call.cameraOff = !call.cameraOff;
  call.local?.getVideoTracks().forEach((track) => (track.enabled = !call.cameraOff));
}

export async function loadHistory(): Promise<void> {
  history.calls = await invoke<CallEntry[]>("core_calls");
}

async function onEvent(event: CallEvent) {
  if (event.kind === "incoming" && !busy()) {
    Object.assign(call, idle(), { id: event.call, contact: event.contact, video: Boolean(event.video), phase: "ringing" });
    offer = event.sdp ?? "";
  } else if (event.call === call.id && event.kind === "answered" && (peer || nativeCall)) {
    clearTimeout(ringTimer);
    media.ringback.stop();
    call.phase = "connecting";
    // A native call took the answer in the core already.
    await peer?.setRemoteDescription({ type: "answer", sdp: event.sdp ?? "" });
  } else if (event.call === call.id && event.kind === "connected" && call.phase !== "active") {
    call.phase = "active";
    call.since = Date.now();
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
  Object.assign(call, idle(), {
    id: current.call,
    contact: current.contact,
    video: current.video,
    outgoing: current.outgoing,
    phase: current.phase,
    muted: current.muted,
    since: current.connectedAt ?? 0,
  });
  offer = current.offer ?? "";
  nativeCall = current.native;
  if (current.phase === "calling") giveUpUnanswered();
  // Without the call screen there is no way to hang up.
  if (live) await router.push(`/call/${current.contact}`);
}

/** Listens to the core's call events and picks up the call it may already have; at start. */
export async function startCalls(): Promise<void> {
  nativeVoice = Boolean(await invoke<boolean>("core_native_calls").catch(() => false));
  await syncCallRouting();
  if (!listening) {
    listening = true;
    await listen<CallEvent>(CALL_EVENT, ({ payload }) => void onEvent(payload));
    // The user may have answered from the notification before this WebView was even there (§66).
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") void applyCallNotification();
    });
  }
  await restoreCall();
  await applyCallNotification();
}
