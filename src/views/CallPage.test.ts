import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonActionSheet, IonIcon } from "@ionic/vue";
import { seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import { chat } from "../core";
import { setLocale } from "../i18n";
import GamePermissions from "../components/GamePermissions.vue";
import CallPage from "./CallPage.vue";
import { pageShape } from "../__tests__/page-shape";
import source from "./CallPage.vue?raw";

const route = { params: { id: "c1" }, query: {} as Record<string, string> };
// The router the page goes back with; `afterEach` is how it knows it got there (`goBack`).
const nav = { back: vi.fn(), replace: vi.fn(), push: vi.fn(), afterEach: vi.fn(() => () => undefined) };
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => nav }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());
// Android's back button (src/back.ts): the handler the call screen takes it over with.
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => (back.handler === handler ? (back.handler = null) : undefined) };
  },
}));
// Ionic's toasts are overlays of the real app; here, what the screen asks of them.
const toast = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

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

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the name in its own direction", () => {
    expect(mount(CallPage, { shallow: true }).find(".ft-call__name").attributes("dir")).toBe("auto");
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

  // 2026-09-29: leaving the call screen keeps the call (the call bar shows it, the camera is
  // held); only the hang-up button ends it. It used to hang up: "no call goes on out of sight".
  it("keeps the call when the screen goes", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    mount(CallPage, { shallow: true }).unmount();
    expect(actions.hangUp).not.toHaveBeenCalled();
  });

  // Found by QA on the emulators (2026-09-29): the back gesture hung up.
  it("goes back from the call screen with Android's back button, and keeps the call", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    mount(CallPage, { shallow: true });
    await flushPromises();
    expect(back.handler).not.toBeNull();
    back.handler?.();
    await flushPromises();
    expect(nav.back).toHaveBeenCalledTimes(1);
    expect(actions.hangUp).not.toHaveBeenCalled();
  });

  it("with nothing behind, Android's back opens the conversation instead of leaving the app", async () => {
    window.history.replaceState({ back: null }, "");
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    mount(CallPage, { shallow: true });
    await flushPromises();
    back.handler?.();
    await flushPromises();
    expect(nav.replace).toHaveBeenCalledWith("/chat/c1");
    expect(actions.hangUp).not.toHaveBeenCalled();
  });

  // Tablets and iPhones have no back button (2026-10-03): an on-screen one does what Android's does.
  const minimize = (wrapper: ReturnType<typeof mount>) => wrapper.find("[aria-label='Back to the chat']");

  it("goes back from the call screen with the on-screen button, and keeps the call", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallPage, { shallow: true });
    await minimize(wrapper).trigger("click");
    await flushPromises();
    expect(nav.back).toHaveBeenCalledTimes(1);
    expect(actions.hangUp).not.toHaveBeenCalled();
  });

  it("with nothing behind, the on-screen button opens the conversation", async () => {
    window.history.replaceState({ back: null }, "");
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallPage, { shallow: true });
    await minimize(wrapper).trigger("click");
    await flushPromises();
    expect(nav.replace).toHaveBeenCalledWith("/chat/c1");
    expect(actions.hangUp).not.toHaveBeenCalled();
  });

  it("names the on-screen back button in the phone's language", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    await setLocale("es");
    try {
      const wrapper = mount(CallPage, { shallow: true });
      expect(wrapper.find("[aria-label='Volver al chat']").exists()).toBe(true);
    } finally {
      await setLocale("en");
    }
  });

  it("has no on-screen back button once the call has ended (the screen leaves by itself)", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ended", outcome: "busy" });
    expect(minimize(mount(CallPage, { shallow: true })).exists()).toBe(false);
  });

  // Ionic keeps the page mounted under the next one: it must act as gone.
  const leaveView = (wrapper: { vm: unknown }) =>
    ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillLeave ?? []).forEach((hook) => hook());

  it("gives Android's back button back once another screen is in front", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallPage, { shallow: true });
    await flushPromises();
    leaveView(wrapper);
    await flushPromises();
    expect(back.handler).toBeNull();
  });

  it("does not go back from another screen when the call ends there", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallPage, { shallow: true });
    leaveView(wrapper);
    call.phase = "ended";
    await nextTick();
    vi.advanceTimersByTime(3000);
    expect(nav.back).not.toHaveBeenCalled();
    expect(nav.replace).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it("does not go back from another screen when the call ended just before leaving", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallPage, { shallow: true });
    call.phase = "ended";
    await nextTick();
    leaveView(wrapper);
    vi.advanceTimersByTime(3000);
    expect(nav.back).not.toHaveBeenCalled();
    vi.useRealTimers();
  });

  it("on the desktop, a voice call has no camera switch", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", video: false, native: false });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[aria-label='Camera']").exists()).toBe(false);
    expect(wrapper.find("[aria-label='Switch camera']").exists()).toBe(false);
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

  // The flip button is always there on a phone (2026-09-29: appearing only with the camera on
  // confused the owner), dimmed and inert while my camera is off.
  it("always shows the flip button on a phone, usable only with my camera on", async () => {
    live();
    const wrapper = mount(CallPage, { shallow: true });
    const flip = () => wrapper.find("[aria-label='Switch camera']");
    expect(flip().exists()).toBe(true);
    expect(flip().attributes("aria-disabled")).toBe("true");
    expect(flip().classes()).toContain("is-waiting");
    await flip().trigger("click");
    expect(actions.switchCamera).not.toHaveBeenCalled();
    call.view = view({ camera: true });
    await nextTick();
    expect(flip().attributes("aria-disabled")).toBe("false");
    expect(flip().classes()).not.toContain("is-waiting");
    await flip().trigger("click");
    expect(actions.switchCamera).toHaveBeenCalled();
  });

  it("keeps the flip button inert while my camera waits for the video line", async () => {
    live({ available: false, camera: true });
    const wrapper = mount(CallPage, { shallow: true });
    expect(wrapper.find("[aria-label='Switch camera']").attributes("aria-disabled")).toBe("true");
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

  // Apple asks for 44 pt tap targets (QA, 2026-10-03): Ionic's round clear button is 38-40.
  it("gives the on-screen back button a 44 px tap target", () => {
    const rule = source.slice(source.indexOf(".ft-call__minimize {"));
    const body = rule.slice(0, rule.indexOf("}"));
    expect(body).toMatch(/min-width:\s*44px/);
    expect(body).toMatch(/min-height:\s*44px/);
  });

  // Arabic reads right to left: no physical sides in the layout (src/CLAUDE.md).
  it("uses no left or right in its styles", () => {
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).not.toMatch(/(^|[\s;{-])(left|right)\s*:/m);
    expect(styles).not.toMatch(/(margin|padding|border)-(left|right)/);
    expect(styles).not.toMatch(/text-align:\s*(left|right)/);
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: the call fills its content, with no header", () => {
    expect(pageShape(mount(CallPage, { shallow: true }))).toEqual(["ion-content"]);
  });
});

// Presenting in a call (docs/plan-presentar-en-llamada.md, 2026-10-08).
describe("CallPage presenting", () => {
  const BOARD = "com.flickertalk.board";
  const PDF = "com.flickertalk.pdfviewer";
  const tool = (id: string, name: string, live: boolean) => ({
    id, name, version: "1.1.0", installedAt: 1,
    asks: { network: [], messages: false, send: "nothing", live: true },
    granted: { network: [], messages: false, send: "nothing", live },
  });
  let tools: unknown[] = [];
  let messages: unknown[] = [];
  let planState = "trial";
  let picked = { path: "/data/uploads/class.pdf", name: "class.pdf", mime: "application/pdf", size: 1_000_000 };
  const hooks: Record<string, () => unknown> = {};
  const invoked: Array<[string, Record<string, unknown> | undefined]> = [];
  const sent = (command: string) => invoked.filter(([one]) => one === command).map(([, args]) => args);
  const view = (patch: Partial<typeof call.view> = {}) => ({ available: true, camera: false, paused: false, facing: "front" as const, remote: false, remotePaused: false, ...patch });
  const active = (patch: Record<string, unknown> = {}) =>
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: false, view: view(), canPresent: true, ...patch });
  const open = async () => {
    const wrapper = mount(CallPage, { shallow: true });
    await flushPromises();
    return wrapper;
  };

  beforeEach(() => {
    seed();
    resetCalls();
    route.query = {};
    nav.push.mockReset();
    toast.create.mockReset().mockResolvedValue({ present: vi.fn() });
    window.history.replaceState({ back: "/chat/c1" }, "");
    document.documentElement.className = "";
    tools = [tool(BOARD, "Board", true), tool(PDF, "PDF viewer", true)];
    messages = [];
    planState = "trial";
    picked = { path: "/data/uploads/class.pdf", name: "class.pdf", mime: "application/pdf", size: 1_000_000 };
    for (const key of Object.keys(hooks)) delete hooks[key];
    invoked.length = 0;
    installTauri((command, args) => {
      invoked.push([command, args]);
      if (hooks[command]) return hooks[command]();
      if (command === "core_plugins") return tools;
      if (command === "core_messages") return messages;
      if (command === "core_plan") return { state: planState, until: 0 };
      if (command === "core_pick_files") return [picked];
      if (command === "core_read_message_file") return { name: "class.pdf", mime: "application/pdf", data: "JVBERi0=" };
      if (command === "core_conversations" || command === "core_requests") return [];
      return undefined;
    });
  });

  it("offers to present only on an active call with a phone that can see it", async () => {
    active({ canPresent: false });
    expect((await open()).find("[data-test='present']").exists()).toBe(false);
    active({ phase: "calling" });
    expect((await open()).find("[data-test='present']").exists()).toBe(false);
    active();
    const button = (await open()).find("[data-test='present']");
    expect(button.attributes("aria-label")).toBe("Present");
  });

  it("presents the whiteboard from Ionic's sheet", async () => {
    active();
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    const sheet = wrapper.findComponent(IonActionSheet);
    expect(sheet.props("isOpen")).toBe(true);
    const buttons = sheet.props("buttons") as Array<{ text: string; icon?: string; handler?: () => void }>;
    expect(buttons.map((one) => one.text)).toEqual(["Whiteboard", "Document (PDF)", "Cancel"]);
    expect(buttons.slice(0, 2).every((one) => typeof one.icon === "string")).toBe(true);
    buttons[0].handler?.();
    await flushPromises();
    expect(actions.presentInCall).toHaveBeenCalledWith("x", BOARD);
  });

  it("sends the chosen PDF as a file of the chat, then presents that message", async () => {
    active();
    hooks.core_send_picked = () => {
      chat("c1")!.messages.push({ id: "m-pdf", mine: true, text: "", time: "", sentAt: 2, kind: "file", file: { name: "class.pdf", size: "1 MB", mime: "application/pdf", progress: 0, state: "sending" } });
    };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[1].handler?.();
    await flushPromises();
    expect(sent("core_pick_files")).toEqual([{ accept: "application/pdf" }]);
    expect(sent("core_send_picked")).toEqual([{ contact: "c1", file: picked }]);
    expect(actions.presentInCall).toHaveBeenCalledWith("x", PDF, "m-pdf");
  });

  it("refuses a PDF too big to present, and sends nothing", async () => {
    active();
    picked = { ...picked, size: 40 * 1024 * 1024 };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[1].handler?.();
    await flushPromises();
    expect(sent("core_send_picked")).toEqual([]);
    expect(actions.presentInCall).not.toHaveBeenCalled();
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "This PDF is too big to present (32 MB at most)" }));
  });

  it("asks once for the live channel before presenting with a tool that lacks it", async () => {
    active();
    tools = [tool(BOARD, "Board", false), tool(PDF, "PDF viewer", false)];
    hooks.core_plugin_grant = () => {
      tools = [tool(BOARD, "Board", true), tool(PDF, "PDF viewer", false)];
    };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    const ask = wrapper.findComponent(GamePermissions);
    expect(ask.props("open")).toBe(true);
    expect(ask.props("body")).toBe("Talk to the same plugin on the other side of the chat");
    expect(actions.presentInCall).not.toHaveBeenCalled();
    ask.vm.$emit("allow");
    await flushPromises();
    expect(sent("core_plugin_grant")[0]).toMatchObject({ plugin: BOARD, granted: { live: true } });
    expect(actions.presentInCall).toHaveBeenCalledWith("x", BOARD);
  });

  it("takes a locked tool to the subscription instead of presenting", async () => {
    active();
    planState = "limited";
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");
    expect(actions.presentInCall).not.toHaveBeenCalled();
  });

  // The mock's `cannotPresent` is the real rule: only `peer_cannot_present` means their app is old.
  it("says their app is too old when the core refuses for that, and that it failed otherwise", async () => {
    active();
    actions.presentInCall.mockRejectedValueOnce(new Error("peer_cannot_present"));
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "Their app can't show presentations yet" }));
    actions.presentInCall.mockRejectedValueOnce(new Error("busy"));
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(toast.create).toHaveBeenLastCalledWith(expect.objectContaining({ message: "The presentation couldn't start" }));
  });

  it("hides Present while the other side presents", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present']").exists()).toBe(false);
    expect(wrapper.find("[data-test='stop-presenting']").exists()).toBe(false);
  });

  it("stops presenting from the top corner, where Present was", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present']").exists()).toBe(false);
    const stop = wrapper.find("[data-test='stop-presenting']");
    expect(stop.attributes("aria-label")).toBe("Stop presenting");
    await stop.trigger("click");
    expect(actions.stopPresenting).toHaveBeenCalledWith("x");
  });
});
