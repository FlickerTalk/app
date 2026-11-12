import { afterEach, beforeEach, describe, expect, it, onTestFinished, vi } from "vitest";
import { effectScope, nextTick, ref } from "vue";
import { actions, call, resetCalls } from "./__tests__/calls-mock";
import { useCallVideo, type CallSurfaces } from "./call-video";

vi.mock("./calls", async () => (await import("./__tests__/calls-mock")).callsMock());

const view = (patch: Partial<typeof call.view> = {}) => ({
  available: true,
  camera: false,
  paused: false,
  facing: "front" as const,
  remote: false,
  remotePaused: false,
  ...patch,
});
const live = (patch: Partial<typeof call.view> = {}, more: Record<string, unknown> = {}) =>
  Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: false, view: view(patch), ...more });
const placed = <T extends Element>(element: T, x: number, y: number, width: number, height: number): T => {
  element.getBoundingClientRect = () => ({ x, y, width, height, left: x, top: y, right: x + width, bottom: y + height, toJSON: () => ({}) }) as DOMRect;
  return element;
};
const media = (patch: Partial<CallSurfaces> = {}): CallSurfaces => ({
  remoteSlot: null,
  localSlot: null,
  remoteVideo: null,
  localVideo: null,
  remoteAudio: null,
  ...patch,
});

function setUp(presenting = false) {
  const current = ref(true);
  const onScreen = ref(true);
  const presentArea = ref(presenting);
  const elements = {
    body: ref<HTMLElement | null>(null),
    surfaces: ref<CallSurfaces | null>(null),
    presentBox: ref<HTMLElement | null>(null),
    controls: ref<{ $el?: Element } | null>(null),
    minimize: ref<{ $el?: Element } | null>(null),
    peer: ref<{ $el?: Element } | null>(null),
  };
  const scope = effectScope();
  const video = scope.run(() => useCallVideo({ current, onScreen, presentArea, ...elements }))!;
  onTestFinished(() => {
    video.detach();
    scope.stop();
  });
  return { video, onScreen, presentArea, ...elements };
}

describe("useCallVideo", () => {
  beforeEach(() => {
    resetCalls();
    document.documentElement.className = "";
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("turns the page see-through only while a native picture shows", async () => {
    live({ camera: true });
    const { video } = setUp();
    expect(video.stage.value).toBe(true);
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(true);
    call.view = view();
    await nextTick();
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
  });

  it("darkens the screen of a video call while it has no picture yet, and only on screen", async () => {
    live({}, { video: true });
    const { video, onScreen } = setUp();
    expect(video.dark.value).toBe(true);
    expect(document.documentElement.classList.contains("ft-dark-screen")).toBe(true);
    onScreen.value = false;
    await nextTick();
    expect(document.documentElement.classList.contains("ft-dark-screen")).toBe(false);
  });

  it("tells the core where the pictures go once attached, and hides them when detached", () => {
    live({ remote: true, camera: true });
    const { video, surfaces } = setUp();
    surfaces.value = media({ remoteSlot: placed(document.createElement("div"), 0, 0, 400, 800), localSlot: placed(document.createElement("div"), 288, 516, 96, 140) });
    video.attach();
    const measure = actions.layoutVideo.mock.calls.at(-1)?.[0] as () => unknown;
    expect(measure()).toEqual({
      remote: { x: 0, y: 0, width: 400, height: 800 },
      local: { x: 288, y: 516, width: 96, height: 140 },
      mirrorLocal: true,
      localRadius: 16,
    });
    video.detach();
    expect(actions.hideVideo).toHaveBeenCalled();
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
  });

  it("measures again when the screen turns or changes size, until detached", () => {
    live({ remote: true });
    const { video } = setUp();
    video.attach();
    actions.layoutVideo.mockClear();
    window.dispatchEvent(new Event("resize"));
    window.dispatchEvent(new Event("orientationchange"));
    expect(actions.layoutVideo).toHaveBeenCalledTimes(2);
    video.detach();
    actions.layoutVideo.mockClear();
    window.dispatchEvent(new Event("resize"));
    expect(actions.layoutVideo).not.toHaveBeenCalled();
  });

  it("hides the pictures while another screen is in front, and shows them again on coming back", async () => {
    live({ remote: true });
    const { video } = setUp();
    video.attach();
    video.wentOut();
    expect(actions.hideVideo).toHaveBeenCalled();
    await nextTick();
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
    actions.layoutVideo.mockClear();
    window.dispatchEvent(new Event("resize"));
    expect(actions.layoutVideo).not.toHaveBeenCalled();
    video.cameIn();
    expect(actions.layoutVideo).toHaveBeenCalled();
  });

  it("says video is unavailable for a moment while the camera cannot be used yet", () => {
    vi.useFakeTimers();
    live({ available: false });
    const { video } = setUp();
    video.camera();
    expect(actions.toggleCamera).not.toHaveBeenCalled();
    expect(video.unavailable.value).toBe(true);
    vi.advanceTimersByTime(4000);
    expect(video.unavailable.value).toBe(false);
    call.view = view();
    video.camera();
    expect(actions.toggleCamera).toHaveBeenCalledTimes(1);
  });

  it("switches the camera only while it runs", () => {
    live();
    const { video } = setUp();
    video.flip();
    expect(actions.switchCamera).not.toHaveBeenCalled();
    call.view = view({ camera: true });
    video.flip();
    expect(actions.switchCamera).toHaveBeenCalledTimes(1);
  });

  it("keeps a presentation clear of the call's buttons, with a hole where their picture shows", () => {
    live({ remote: true });
    const { video, body, controls, presentBox, surfaces } = setUp(true);
    expect(video.remoteThumb.value).toBe(true);
    body.value = placed(document.createElement("div"), 0, 0, 400, 800);
    controls.value = { $el: placed(document.createElement("div"), 40, 703, 320, 58) };
    presentBox.value = placed(document.createElement("section"), 0, 60, 400, 600);
    surfaces.value = media({ remoteSlot: placed(document.createElement("div"), 290, 500, 96, 140) });
    video.attach();
    expect(video.presentStyle.value).toMatchObject({ bottom: "97px" });
    expect(video.presentStyle.value?.clipPath).toContain("290px 440px, 386px 440px");
  });

  it("plays the WebView's streams on its media elements", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: true, remote: new MediaStream(), local: new MediaStream() });
    const { surfaces } = setUp();
    const remote = document.createElement("video");
    const local = document.createElement("video");
    surfaces.value = media({ remoteVideo: remote, localVideo: local });
    await nextTick();
    expect(remote.srcObject).toBe(call.remote);
    expect(local.srcObject).toBe(call.local);
  });
});
