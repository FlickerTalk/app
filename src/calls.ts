import { markRaw, reactive } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { storedCallRouting, syncCallRouting } from "./preferences";
import { showCallScreen } from "./call-screen";
import { router } from "./router";

export type CallPhase = "idle" | "calling" | "ringing" | "connecting" | "active" | "ended";
export type CallOutcome = "answered" | "missed" | "declined" | "busy" | "cancelled" | "unreachable" | "failed";

export interface CallVideo {
  available: boolean;
  camera: boolean;
  paused: boolean;
  facing: "front" | "back";
  remote: boolean;
  remotePaused: boolean;
}

// CSS pixels, as the bridge's VideoLayout
export interface VideoRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
export interface VideoLayout {
  remote: VideoRect | null;
  local: VideoRect | null;
  mirrorLocal: boolean;
  localRadius: number;
}

export type CallRoute = "receiver" | "speaker" | "headset";

const CALL_ROUTES: readonly string[] = ["receiver", "speaker", "headset"];

export interface Presenting {
  plugin: string;
  file?: string;
  by: "me" | "them";
}

export interface CallState {
  id: string;
  contact: string;
  video: boolean;
  outgoing: boolean;
  phase: CallPhase;
  outcome: CallOutcome | null;
  since: number;
  muted: boolean;
  speaker: boolean;
  route: CallRoute | null;
  cameraOff: boolean;
  local: MediaStream | null;
  remote: MediaStream | null;
  native: boolean;
  view: CallVideo;
  cameraDenied: boolean;
  cameraFailed: boolean;
  presenting: Presenting | null;
  /** both apps on call media version 2 */
  canPresent: boolean;
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
  kind: "incoming" | "answering" | "answered" | "connected" | "muted" | "ended" | "video" | "camera_failed" | "presenting" | "route";
  video?: boolean;
  /** the core answered it already, before its offer came */
  answered?: boolean;
  sdp?: string;
  outcome?: CallOutcome;
  muted?: boolean;
  canPresent?: boolean;
  plugin?: string;
  file?: string;
  by?: "me" | "them";
  route?: CallRoute;
}

interface CurrentCall {
  call: string;
  contact: string;
  // a boolean from an older core
  video: boolean | CallVideo;
  outgoing: boolean;
  phase: "calling" | "ringing" | "connecting" | "active";
  offer?: string;
  native: boolean;
  muted: boolean;
  connectedAt?: number;
  canPresent?: boolean;
  presenting?: Presenting | null;
  route?: CallRoute;
}

export const CALL_EVENT = "ft://call";
export const RING_LIMIT = 45_000;
const GATHER_LIMIT = 3_000;

interface Tone {
  start(): void;
  stop(): void;
}

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
  route: null,
  cameraOff: false,
  local: null,
  remote: null,
  native: false,
  view: noVideo(),
  cameraDenied: false,
  cameraFailed: false,
  presenting: null,
  canPresent: false,
});

export const call = reactive<CallState>(idle());
export const history = reactive({ calls: [] as CallEntry[] });

let peer: RTCPeerConnection | null = null;
let offer = "";
let ringTimer: ReturnType<typeof setTimeout> | undefined;
let listening = false;
let nativeCalls = false;
let nativeCall = false;
// a core read taken before the last presenting event says nothing about what is presented
let presentingHeard = 0;

function setNative(native: boolean) {
  nativeCall = native;
  call.native = native;
}

const cameraDenied = (error: unknown) => String(error).includes("camera_denied");

// WebKit may change the audio session, which CallKit and the native voice own
const webViewMayPlay = () => !(nativeCall && /iPhone|iPad/.test(navigator.userAgent));

// the speaker override beats headphones, so leave the route to the phone
function goLive() {
  call.phase = "active";
  call.since = Date.now();
}

function followRoute(route: string | undefined) {
  if (!route || !CALL_ROUTES.includes(route)) return;
  call.route = route as CallRoute;
  call.speaker = route === "speaker";
}

function busy(): boolean {
  return call.phase !== "idle" && call.phase !== "ended";
}

const isTurn = (server: RTCIceServer) => [server.urls].flat().some((url) => url.startsWith("turn"));

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

function release() {
  cancelAnimationFrame(layoutFrame);
  layoutFrame = 0;
  clearTimeout(ringTimer);
  media.ringback.stop();
  call.local?.getTracks().forEach((track) => track.stop());
  peer?.close();
  peer = null;
}

function finish() {
  release();
  call.phase = "ended";
  call.presenting = null;
  call.canPresent = false;
}

export function reset() {
  release();
  offer = "";
  sentLayout = undefined;
  Object.assign(call, idle());
  setNative(false);
}

export async function startCall(contact: string, video: boolean): Promise<void> {
  if (busy()) throw new Error("already in a call");
  Object.assign(call, idle(), { contact, video, outgoing: true, phase: "calling", speaker: video });
  setNative(nativeCalls);
  try {
    if (nativeCall) {
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

function giveUpUnanswered() {
  clearTimeout(ringTimer);
  ringTimer = setTimeout(() => {
    if (call.phase === "calling") void hangUp();
  }, RING_LIMIT);
}

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

async function answerNativeCall() {
  try {
    await invoke("core_call_answer_native", { call: call.id, routing: storedCallRouting() });
  } catch (error) {
    if (!call.video || !cameraDenied(error)) throw error;
    call.cameraDenied = true;
  }
}

async function showCall(): Promise<void> {
  if (call.contact) await showCallScreen(router, `/call/${call.contact}`);
}

async function answeredByTheCore(): Promise<void> {
  call.phase = "connecting";
  setNative(true);
  await showCall();
}

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

export function toggleSpeaker() {
  call.speaker = !call.speaker;
  void invoke("core_call_speaker", { on: call.speaker }).catch(() => undefined);
}

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
    await followCore();
  }
}

export async function switchCamera(): Promise<void> {
  if (!nativeCall) return;
  const view = await invoke<CallVideo>("core_call_switch_camera", { call: call.id }).catch(() => null);
  if (view) applyVideo(view);
}

export const CANNOT_PRESENT = "peer_cannot_present";

export function cannotPresent(error: unknown): boolean {
  return String(error instanceof Error ? error.message : error) === CANNOT_PRESENT;
}

function presentingOf(said: { plugin?: string | null; file?: string | null; by?: "me" | "them" | null } | null | undefined): Presenting | null {
  if (!said?.plugin || !said.by) return null;
  return said.file ? { plugin: said.plugin, file: said.file, by: said.by } : { plugin: said.plugin, by: said.by };
}

export async function presentInCall(callId: string, plugin: string, file?: string): Promise<void> {
  try {
    await invoke("core_call_present", { call: callId, plugin, file: file ?? null });
  } finally {
    // both pressed at once, end on what the core keeps
    await followCore();
  }
}

export async function stopPresenting(callId: string): Promise<void> {
  await invoke("core_call_present_stop", { call: callId });
}

const anyVideo = (view: CallVideo) => view.camera || view.remote;

// the native views may only exist now, lay them out again
function applyVideo(update: Partial<CallVideo>) {
  const view: CallVideo = { ...call.view };
  for (const key of Object.keys(view) as (keyof CallVideo)[]) {
    if (update[key] !== undefined) (view as unknown as Record<string, unknown>)[key] = update[key];
  }
  call.view = view;
  sentLayout = undefined;
}

async function followCore(): Promise<void> {
  const id = call.id;
  if (!id || !busy()) return;
  const heard = presentingHeard;
  const current = await invoke<CurrentCall | null>("core_current_call").catch(() => null);
  if (!current || current.call !== id || call.id !== id) return;
  const live = current.phase === "connecting" || current.phase === "active";
  if (call.phase === "ringing" && live) await answeredByTheCore();
  if (typeof current.video === "object" && current.video) applyVideo(current.video);
  call.canPresent = Boolean(current.canPresent);
  followRoute(current.route);
  // a presenting event heard meanwhile is newer than this read
  if (heard === presentingHeard) call.presenting = presentingOf(current.presenting);
}

export function rectOf(element: Element | null | undefined): VideoRect | null {
  if (!element) return null;
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
}

let layoutFrame = 0;
let measureLayout: (() => VideoLayout | null) | null = null;
let sentLayout: string | undefined;

function sendLayout(layout: VideoLayout | null) {
  const said = JSON.stringify(layout);
  if (said === sentLayout) return;
  sentLayout = said;
  void invoke("core_call_video_layout", { layout }).catch(() => undefined);
}

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
  // a late event after the call ended brings nothing back
  const late = event.call === call.id && call.phase === "ended";
  if (event.call === call.id && event.kind === "connected" && !late) call.canPresent = Boolean(event.canPresent);
  if (event.kind === "incoming" && !busy()) {
    const video = Boolean(event.video);
    Object.assign(call, idle(), { id: event.call, contact: event.contact, video, phase: "ringing", speaker: video });
    offer = event.sdp ?? "";
    if (event.answered) await answeredByTheCore();
  } else if (event.call === call.id && event.kind === "answering" && call.phase === "ringing") {
    await answeredByTheCore();
  } else if (event.call === call.id && event.kind === "answered" && (peer || nativeCall)) {
    clearTimeout(ringTimer);
    media.ringback.stop();
    call.phase = "connecting";
    // a native call has the answer in the core already
    await peer?.setRemoteDescription({ type: "answer", sdp: event.sdp ?? "" });
    await showCall();
  } else if (event.call === call.id && event.kind === "connected" && call.phase !== "active") {
    // answered by CallKit while the app only showed it ringing
    goLive();
    await showCall();
  } else if (event.call === call.id && event.kind === "video" && nativeCall) {
    applyVideo(event);
    if (call.view.camera) call.cameraFailed = false;
  } else if (event.call === call.id && event.kind === "camera_failed" && nativeCall) {
    applyVideo({ camera: false });
    call.cameraFailed = true;
  } else if (event.call === call.id && event.kind === "muted") {
    call.muted = Boolean(event.muted);
  } else if (event.call === call.id && event.kind === "route" && !late) {
    followRoute(event.route);
  } else if (event.call === call.id && event.kind === "presenting" && !late) {
    presentingHeard += 1;
    call.presenting = presentingOf(event);
  } else if (event.call === call.id && event.kind === "ended") {
    if (call.phase !== "ended") finish();
    call.outcome = event.outcome ?? null;
  }
  if (event.kind === "incoming" || event.kind === "ended") await loadHistory();
}

// CallKit may have answered before this WebView listened
async function restoreCall(): Promise<void> {
  const current = await invoke<CurrentCall | null>("core_current_call").catch(() => null);
  if (!current || busy()) return;
  const live = current.phase === "connecting" || current.phase === "active";
  if (!current.native && current.phase !== "ringing") {
    await invoke("core_call_end", { call: current.call, failed: true }).catch(() => undefined);
    return;
  }
  // an older core only says whether it's a video call
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
    canPresent: Boolean(current.canPresent),
    presenting: presentingOf(current.presenting),
  });
  followRoute(current.route);
  offer = current.offer ?? "";
  setNative(current.native);
  if (current.phase === "calling") giveUpUnanswered();
  // the call screen is the only way to hang up
  if (live) await showCall();
}

export async function startCalls(): Promise<void> {
  nativeCalls = Boolean(await invoke<boolean>("core_native_calls").catch(() => false));
  await syncCallRouting();
  if (!listening) {
    listening = true;
    await listen<CallEvent>(CALL_EVENT, ({ payload }) => void onEvent(payload));
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "visible") void followCore();
    });
  }
  await restoreCall();
}
