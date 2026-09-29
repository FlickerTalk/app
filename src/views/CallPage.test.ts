import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon } from "@ionic/vue";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallPage from "./CallPage.vue";
import source from "./CallPage.vue?raw";

const route = { params: { id: "c1" }, query: {} as Record<string, string> };
const nav = { back: vi.fn(), replace: vi.fn() };
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => nav }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

describe("CallPage", () => {
  // Every screen mounted here watches the same call: one left mounted would react to the next test.
  enableAutoUnmount(afterEach);
  beforeEach(() => {
    seed();
    resetCalls();
    route.query = {};
    nav.back.mockReset();
    nav.replace.mockReset();
    window.history.replaceState({ back: "/chat/c1" }, "");
  });

  it("calls the contact on the screen", () => {
    mount(CallPage, { shallow: true });
    expect(actions.startCall).toHaveBeenCalledWith("c1", false);
    route.query = { video: "1" };
    resetCalls();
    mount(CallPage, { shallow: true });
    expect(actions.startCall).toHaveBeenCalledWith("c1", true);
  });

  // Answering an incoming call opens this screen on a call that is already going on.
  it("shows the call going on instead of placing another", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "connecting", video: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(actions.startCall).not.toHaveBeenCalled();
    expect(wrapper.find("[data-test='video']").exists()).toBe(true);
    expect(wrapper.text()).toContain("Connecting…");
  });

  it("shows who is being called and how to hang up", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    expect(wrapper.text()).toContain("Calling…");
    await wrapper.find("[aria-label='Hang up']").trigger("click");
    expect(actions.hangUp).toHaveBeenCalled();
  });

  it("shows the call's clock once it is connected", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() - 65_000 });
    expect(mount(CallPage, { shallow: true }).text()).toContain("01:05");
  });

  it("says how the call ended", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ended", outcome: "busy" });
    expect(mount(CallPage, { shallow: true }).text()).toContain("Busy");
  });

  it("mutes and turns the camera off", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: true, muted: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[aria-label='Mute']").attributes("aria-pressed")).toBe("true");
    await wrapper.find("[aria-label='Mute']").trigger("click");
    await wrapper.find("[aria-label='Camera']").trigger("click");
    expect(actions.toggleMute).toHaveBeenCalled();
    expect(actions.toggleCamera).toHaveBeenCalled();
  });

  // Speaker or receiver (2026-09-28): an icon, named for screen readers.
  it("switches the speaker", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: false, speaker: false });
    const wrapper = mount(CallPage, { shallow: true });
    const speaker = wrapper.find("[aria-label='Speaker']");
    expect(speaker.attributes("aria-pressed")).toBe("false");
    await speaker.trigger("click");
    expect(actions.toggleSpeaker).toHaveBeenCalled();
  });

  // Until the contact's video arrives there is no empty player (Android paints a grey poster):
  // their avatar shows instead.
  it("shows the contact's video only once it arrives", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling", video: true, remote: null });
    const waiting = mount(CallPage, { shallow: true });
    expect(waiting.find(".ft-call__remote").exists()).toBe(false);
    expect(waiting.findComponent({ name: "Avatar" }).exists()).toBe(true);
    call.remote = new MediaStream();
    const live = mount(CallPage, { shallow: true });
    expect(live.find(".ft-call__remote").exists()).toBe(true);
    expect(live.findComponent({ name: "Avatar" }).exists()).toBe(false);
  });

  // One icon element per state: Ionicons loads icons asynchronously, and a reused element could
  // end up showing the previous state (a crossed microphone while it is on).
  it("draws a fresh icon for each state of the toggles", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: true, muted: false });
    const wrapper = mount(CallPage, { shallow: true });
    const before = wrapper.find("[aria-label='Mute']").findComponent(IonIcon).element;
    call.muted = true;
    await wrapper.vm.$nextTick();
    expect(wrapper.find("[aria-label='Mute']").findComponent(IonIcon).element).not.toBe(before);
  });

  it("shows the video area only on video calls", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling", video: false });
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(false);
    call.video = true;
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(true);
  });

  // Found on the tablet (2026-09-28): a call nobody answered ended as "Unreachable" and the
  // screen stayed there; hanging up did nothing, since there was no call left to end.
  it("hanging up leaves the screen at once, even after the call ended", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ended", outcome: "unreachable" });
    const wrapper = mount(CallPage, { shallow: true });
    await wrapper.find("[aria-label='Hang up']").trigger("click");
    await flushPromises();
    expect(nav.back).toHaveBeenCalledTimes(1);
  });

  // Opened with nothing behind it (from a notification, or the page itself reloaded), going
  // back would do nothing: the conversation with the contact opens instead.
  it("with nowhere to go back to, leaving opens the conversation", async () => {
    window.history.replaceState({ back: null }, "");
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const wrapper = mount(CallPage, { shallow: true });
    await wrapper.find("[aria-label='Hang up']").trigger("click");
    await flushPromises();
    expect(nav.replace).toHaveBeenCalledWith("/chat/c1");
    expect(nav.back).not.toHaveBeenCalled();
  });

  it("leaves only once when the call ends after hanging up", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const wrapper = mount(CallPage, { shallow: true });
    await wrapper.find("[aria-label='Hang up']").trigger("click");
    call.phase = "ended";
    await nextTick();
    vi.advanceTimersByTime(3000);
    await flushPromises();
    expect(nav.back).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("leaves by itself a moment after the call ended", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    mount(CallPage, { shallow: true });
    call.phase = "ended";
    await nextTick();
    vi.advanceTimersByTime(3000);
    expect(nav.back).toHaveBeenCalledTimes(1);
    vi.useRealTimers();
  });

  it("does not go back from elsewhere once the screen is gone", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const wrapper = mount(CallPage, { shallow: true });
    call.phase = "ended";
    await nextTick();
    wrapper.unmount();
    vi.advanceTimersByTime(3000);
    expect(nav.back).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it("on the desktop, a voice call has no camera switch", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: false, native: false });
    expect(mount(CallPage, { shallow: true }).find("[aria-label='Camera']").exists()).toBe(false);
  });
});

// Native video (2026-09-29, docs/video-nativo.md): on a phone every call can go from voice to
// video and back at any moment. The pictures are native views under the WebView; this screen
// leaves see-through holes where they go and says where those are.
describe("CallPage with native video", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    route.query = {};
    window.history.replaceState({ back: "/chat/c1" }, "");
    document.documentElement.className = "";
  });

  const view = (patch: Partial<typeof call.view> = {}) => ({
    available: true,
    camera: false,
    paused: false,
    facing: "front" as const,
    remote: false,
    remotePaused: false,
    ...patch,
  });
  const live = (patch: Partial<typeof call.view> = {}) =>
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: false, view: view(patch) });
  const lastMeasure = () => actions.layoutVideo.mock.calls.at(-1)?.[0] as () => Record<string, unknown>;

  it("shows the voice/video switch at all times, even on a voice call", () => {
    live();
    const toggle = mount(CallPage, { shallow: true }).find("[aria-label='Camera']");
    expect(toggle.exists()).toBe(true);
    expect(toggle.attributes("aria-pressed")).toBe("false");
    expect(toggle.attributes("aria-disabled")).toBe("false");
  });

  it("turns my camera on and off from the switch", async () => {
    live();
    const wrapper = mount(CallPage, { shallow: true });
    await wrapper.find("[aria-label='Camera']").trigger("click");
    expect(actions.toggleCamera).toHaveBeenCalled();
    call.view = view({ camera: true });
    await nextTick();
    expect(wrapper.find("[aria-label='Camera']").attributes("aria-pressed")).toBe("true");
  });

  it("offers to switch between the front and back camera only with my camera on", async () => {
    live();
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[aria-label='Switch camera']").exists()).toBe(false);
    call.view = view({ camera: true });
    await nextTick();
    await wrapper.find("[aria-label='Switch camera']").trigger("click");
    expect(actions.switchCamera).toHaveBeenCalled();
  });

  // Before the call connects the core cannot open the camera yet: the switch waits to turn it on.
  // A camera already wanted can always be turned off (2026-09-29: it used to be stuck on).
  it("keeps the switch waiting until the call connects, except to turn the camera off", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling", native: true, video: false, view: view({ available: false }) });
    const off = mount(CallPage, { shallow: true }).find("[aria-label='Camera']");
    expect(off.attributes("aria-pressed")).toBe("false");
    expect(off.attributes("aria-disabled")).toBe("true");
    Object.assign(call, { video: true, view: view({ available: false, camera: true }) });
    const on = mount(CallPage, { shallow: true }).find("[aria-label='Camera']");
    expect(on.attributes("aria-pressed")).toBe("true");
    expect(on.attributes("aria-disabled")).toBe("false");
  });

  it("says video is not available on a call without a video line", async () => {
    live({ available: false });
    const wrapper = mount(CallPage, { shallow: true });
    const toggle = wrapper.find("[aria-label='Camera']");
    expect(toggle.attributes("aria-disabled")).toBe("true");
    await toggle.trigger("click");
    expect(actions.toggleCamera).not.toHaveBeenCalled();
    expect(wrapper.text()).toContain("Video isn't available on this call");
  });

  // Each side owns its camera: theirs coming on only invites me to turn mine on.
  it("invites me to turn my camera on when theirs comes on", async () => {
    live({ remote: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.text()).toContain("Turn on your camera");
    expect(wrapper.find("[aria-label='Camera']").classes()).toContain("is-invite");
    expect(actions.toggleCamera).not.toHaveBeenCalled();
    call.view = view({ remote: true, camera: true });
    await nextTick();
    expect(wrapper.text()).not.toContain("Turn on your camera");
  });

  it("says their camera is paused while their phone is locked or in the background", () => {
    live({ remote: true, remotePaused: true });
    const paused = mount(CallPage, { shallow: true }).find("[data-test='remote-paused']");
    expect(paused.exists()).toBe(true);
    expect(paused.text()).toContain("Camera paused");
  });

  it("says why the camera did not turn on, and how to allow it", () => {
    live();
    call.cameraDenied = true;
    const denied = mount(CallPage, { shallow: true }).find("[data-test='camera-denied']");
    expect(denied.exists()).toBe(true);
    expect(denied.attributes("role")).toBe("alert");
    expect(denied.text()).toContain("Settings");
  });

  it("leaves see-through holes for the native pictures, and no WebView video", async () => {
    live({ remote: true, camera: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[data-test='remote-slot']").exists()).toBe(true);
    expect(wrapper.find("[data-test='local-slot']").classes()).toContain("is-thumb");
    expect(wrapper.find("video").exists()).toBe(false);
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(true);
    call.view = view();
    await nextTick();
    expect(wrapper.find("[data-test='remote-slot']").exists()).toBe(false);
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
  });

  // Found by QA on the emulators (2026-09-29): a video call whose camera never started left the
  // page see-through with nothing under it, a blank white screen.
  it("goes see-through only while there is a picture to show", async () => {
    live({ available: false, camera: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[data-test='local-slot']").exists()).toBe(false);
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
    call.view = view({ camera: true });
    await nextTick();
    expect(wrapper.find("[data-test='local-slot']").exists()).toBe(true);
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(true);
  });

  it("has a dark background on a video call without pictures, never the page's white", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: true, view: view() });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find(".ft-call").classes()).toContain("is-dark");
    // See-through, the native views paint it; dark, it would hide them.
    call.view = view({ camera: true });
    await nextTick();
    expect(wrapper.find(".ft-call").classes()).not.toContain("is-dark");
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).toMatch(/\.ft-call\.is-dark\s*\{[^}]*--background:\s*#0/);
  });

  it("keeps a voice call on the page's own background", () => {
    live();
    expect(mount(CallPage, { shallow: true }).find(".ft-call").classes()).not.toContain("is-dark");
  });

  it("always lets me turn my camera off, even while it waits", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: true, view: view({ available: false, camera: true }) });
    const wrapper = mount(CallPage, { shallow: true });
    const toggle = wrapper.find("[aria-label='Camera']");
    expect(toggle.attributes("aria-disabled")).toBe("false");
    await toggle.trigger("click");
    expect(actions.toggleCamera).toHaveBeenCalled();
    expect(wrapper.text()).not.toContain("Video isn't available on this call");
  });

  it("says when the camera could not start, and the call goes on", () => {
    live();
    call.cameraFailed = true;
    const wrapper = mount(CallPage, { shallow: true });
    const failed = wrapper.find("[data-test='camera-failed']");
    expect(failed.exists()).toBe(true);
    expect(failed.attributes("role")).toBe("alert");
    expect(failed.text()).toContain("The camera couldn't start");
    expect(wrapper.find("[aria-label='Hang up']").exists()).toBe(true);
  });

  // Until their picture comes, mine fills the screen.
  it("fills the screen with my picture while theirs is off", () => {
    live({ camera: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[data-test='remote-slot']").exists()).toBe(false);
    expect(wrapper.find("[data-test='local-slot']").classes()).not.toContain("is-thumb");
  });

  it("tells the core where the pictures go when it shows them", () => {
    live({ remote: true, camera: true });
    mount(CallPage, { shallow: true });
    expect(actions.layoutVideo).toHaveBeenCalled();
    const layout = lastMeasure()();
    expect(layout).toEqual({
      remote: { x: 0, y: 0, width: 0, height: 0 },
      local: { x: 0, y: 0, width: 0, height: 0 },
      mirrorLocal: true,
      localRadius: 16,
    });
  });

  it("says no picture goes anywhere on a voice call, and mirrors only the front camera", async () => {
    live();
    mount(CallPage, { shallow: true });
    expect(lastMeasure()()).toMatchObject({ remote: null, local: null });
    call.view = view({ camera: true, facing: "back" });
    await nextTick();
    expect(lastMeasure()()).toMatchObject({ local: { x: 0, y: 0, width: 0, height: 0 }, mirrorLocal: false, localRadius: 0 });
  });

  it("measures again when the screen turns or changes size", () => {
    live({ remote: true });
    mount(CallPage, { shallow: true });
    actions.layoutVideo.mockClear();
    window.dispatchEvent(new Event("resize"));
    expect(actions.layoutVideo).toHaveBeenCalled();
    actions.layoutVideo.mockClear();
    window.dispatchEvent(new Event("orientationchange"));
    expect(actions.layoutVideo).toHaveBeenCalled();
  });

  it("drags my picture and tells the core where it went", async () => {
    live({ remote: true, camera: true });
    const wrapper = mount(CallPage, { shallow: true });
    const thumb = wrapper.find("[data-test='local-slot']");
    actions.layoutVideo.mockClear();
    await thumb.trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 1 });
    await thumb.trigger("pointermove", { clientX: 200, clientY: 500, pointerId: 1 });
    await thumb.trigger("pointerup", { clientX: 200, clientY: 500, pointerId: 1 });
    expect(thumb.attributes("style")).toContain("translate(-100px, -100px)");
    expect(actions.layoutVideo).toHaveBeenCalled();
  });

  it("hides the pictures when the call screen goes", () => {
    live({ remote: true });
    mount(CallPage, { shallow: true }).unmount();
    expect(actions.hideVideo).toHaveBeenCalled();
    expect(document.documentElement.classList.contains("ft-call-video")).toBe(false);
  });

  // Arabic reads right to left: no physical sides in the layout (src/CLAUDE.md).
  it("uses no left or right in its styles", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).not.toMatch(/(^|[\s;{-])(left|right)\s*:/m);
    expect(styles).not.toMatch(/(margin|padding|border)-(left|right)/);
    expect(styles).not.toMatch(/text-align:\s*(left|right)/);
  });
});
