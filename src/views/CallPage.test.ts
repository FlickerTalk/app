import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonActionSheet, IonIcon } from "@ionic/vue";
import { seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import { chat, loadMessages } from "../core";
import { installed, refreshPlugins } from "../plugins";
import { setLocale } from "../i18n";
import { callScreenGone } from "../call-screen";
import GamePermissions from "../components/GamePermissions.vue";
import PluginSheet from "../components/PluginSheet.vue";
import CallPage from "./CallPage.vue";
import { pageShape } from "../__tests__/page-shape";
import source from "./CallPage.vue?raw";

const route = { params: { id: "c1" }, query: {} as Record<string, string> };
// The router the page goes back with; `afterEach` is how it knows it got there (`goBack`).
const nav = {
  back: vi.fn(),
  replace: vi.fn(),
  push: vi.fn(),
  afterEach: vi.fn(() => () => undefined),
  currentRoute: { value: { path: "/call/c1" } },
};
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
// The real list of plugins, whose read can be made to fail.
const reading = vi.hoisted(() => ({ fails: false }));
vi.mock("../plugins", async (importOriginal) => {
  const real = await importOriginal<typeof import("../plugins")>();
  return { ...real, refreshPlugins: () => (reading.fails ? Promise.reject(new Error("unreadable")) : real.refreshPlugins()) };
});
// Ionic's toasts are overlays of the real app; here, what the screen asks of them.
const toast = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

// Every screen mounted here watches the same call: one left mounted would react to the next test.
enableAutoUnmount(afterEach);

describe("CallPage", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    route.query = {};
    nav.back.mockReset();
    nav.replace.mockReset();
    window.history.replaceState({ back: "/chat/c1" }, "");
  });

  // 2026-10-09: the screen is on the page until it unmounts, after the transition that takes it
  // away; going back to the call waits for that (the call bar tapped while it left: a blank page).
  it("is on the page from its mount to its unmount", async () => {
    let gone = false;
    const wrapper = mount(CallPage, { shallow: true });
    void callScreenGone().then(() => (gone = true));
    await flushPromises();
    expect(gone).toBe(false);
    wrapper.unmount();
    await flushPromises();
    expect(gone).toBe(true);
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
  const pdfView = (id: string, state: string, outgoing = false) => ({
    id, outgoing, text: "", sentAt: 1, state: "delivered",
    file: { name: "class.pdf", size: 15_000_000, mime: "application/pdf", progress: state === "done" ? 1 : 0, state, path: "/data/files/class.pdf" },
  });
  /** The stub sheet has no `close()`: this one says goodbye at once, or, with `wait`, when told to. */
  const goodbye = (wrapper: VueWrapper, wait = false) => {
    const sheet = wrapper.findComponent(PluginSheet);
    const close = vi.fn(async () => void (wait || sheet.vm.$emit("closed")));
    Object.assign(sheet.vm, { close });
    return close;
  };
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
  // The plugins this phone has are read first, as the app has them by the time a call is on: none
  // left over from the test before.
  const open = async (attach = false) => {
    await refreshPlugins();
    // `attach`: in the document, since happy-dom computes no style for a detached element (`isVisible`).
    const wrapper = mount(CallPage, { shallow: true, attachTo: attach ? document.body : undefined });
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
    reading.fails = false;
    installTauri((command, args) => {
      invoked.push([command, args]);
      if (hooks[command]) return hooks[command]();
      if (command === "core_plugins") return tools;
      if (command === "core_messages") return messages;
      if (command === "core_plan") return { state: planState, until: 0 };
      if (command === "core_pick_files") return [picked];
      if (command === "core_read_message_file") return { name: "class.pdf", mime: "application/pdf", data: "JVBERi0=" };
      if (command === "core_file_belongs_to") return true;
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

  // The core keeps a cleaned name ("Tema 1: Fracciones.pdf" → "Tema 1_ Fracciones.pdf"): the
  // message is found by its size, never by the name the picker gave.
  it("sends the chosen PDF as a file of the chat, then presents that message", async () => {
    active();
    picked = { ...picked, name: "Tema 1: Fracciones.pdf" };
    hooks.core_send_picked = () => {
      chat("c1")!.messages.push({ id: "m-pdf", mine: true, text: "", time: "", sentAt: 2, kind: "file", file: { name: "Tema 1_ Fracciones.pdf", size: "1 MB", bytes: 1_000_000, mime: "application/pdf", progress: 0, state: "sending" } });
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

  // The client's lock check can be stale; the core has the last word (`needs_subscription`).
  it("takes a refusal for the subscription to Premium, without a toast", async () => {
    active();
    actions.presentInCall.mockRejectedValueOnce(new Error("needs_subscription"));
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");
    expect(toast.create).not.toHaveBeenCalled();
  });

  it("sends the PDF once however often Present is tapped while it is on its way", async () => {
    active();
    let arrive = () => undefined as void;
    hooks.core_send_picked = () =>
      new Promise<void>((done) => {
        arrive = () => {
          chat("c1")!.messages.push({ id: "m-pdf", mine: true, text: "", time: "", sentAt: 2, kind: "file", file: { name: "class.pdf", size: "1 MB", bytes: 1_000_000, mime: "application/pdf", progress: 0, state: "sending" } });
          done();
        };
      });
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    const pdf = () => (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[1].handler?.();
    pdf();
    await flushPromises();
    expect(wrapper.find("[data-test='present']").attributes("disabled")).toBe("true");
    pdf();
    await flushPromises();
    arrive();
    await flushPromises();
    expect(sent("core_send_picked")).toHaveLength(1);
    expect(actions.presentInCall).toHaveBeenCalledTimes(1);
    expect(wrapper.find("[data-test='present']").attributes("disabled")).toBe("false");
  });

  it("says so when stopping fails", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    actions.stopPresenting.mockRejectedValueOnce(new Error("busy"));
    const wrapper = await open();
    await wrapper.find("[data-test='stop-presenting']").trigger("click");
    await flushPromises();
    expect(toast.create).toHaveBeenCalledTimes(1);
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "The presentation couldn't start" }));
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

  it("opens the presenter's tool over the call, leading", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const sheet = (await open()).findComponent(PluginSheet);
    expect(sheet.props()).toMatchObject({ plugin: { id: BOARD, name: "Board" }, contact: "c1", live: true, presenting: "lead" });
    expect(sheet.props("file")).toBeUndefined();
  });

  it("leads with the PDF it sent, read at once with no question about whose it is", async () => {
    messages = [pdfView("m1", "done", true)];
    active({ presenting: { plugin: PDF, file: "m1", by: "me" } });
    const wrapper = await open();
    expect(sent("core_read_message_file")).toEqual([{ message: "m1" }]);
    expect(sent("core_file_belongs_to")).toEqual([]);
    expect(wrapper.findComponent(PluginSheet).props()).toMatchObject({ presenting: "lead", file: { name: "class.pdf", mime: "application/pdf", data: "JVBERi0=" } });
  });

  it("follows their presentation with the PDF of the chat, and says who presents", async () => {
    messages = [pdfView("m1", "done")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(sent("core_file_belongs_to")).toEqual([{ file: "m1", contact: "c1" }]);
    expect(sent("core_read_message_file")).toEqual([{ message: "m1" }]);
    expect(wrapper.findComponent(PluginSheet).props()).toMatchObject({ presenting: "follow", file: { name: "class.pdf", mime: "application/pdf", data: "JVBERi0=" } });
    expect(wrapper.find("[data-test='presenter']").text()).toBe("Maria López is presenting");
  });

  // A file of another chat (its message came after the presentation did): never read, never shown.
  it("reads no file the core says is not of this chat, and says the presentation could not start", async () => {
    messages = [pdfView("m1", "done")];
    hooks.core_file_belongs_to = () => false;
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(sent("core_read_message_file")).toEqual([]);
    expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
    expect(wrapper.find("[data-test='present-failed']").text()).toBe("The presentation couldn't start");
  });

  it("asks to accept a PDF over the automatic download, and opens it once it is here", async () => {
    messages = [pdfView("m1", "waiting")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
    const accept = wrapper.find("[data-test='present-accept']");
    expect(accept.text()).toContain("Accept the file to see the presentation");
    await accept.trigger("click");
    expect(sent("core_accept_file")).toEqual([{ message: "m1" }]);
    messages = [pdfView("m1", "done")];
    await loadMessages("c1");
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("presenting")).toBe("follow");
  });

  it("says at once that their PDF is too big to present, with nothing to accept", async () => {
    const big = pdfView("m1", "waiting");
    messages = [{ ...big, file: { ...big.file, size: 40_000_000 } }];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present-accept']").exists()).toBe(false);
    expect(wrapper.find("[data-test='present-too-big']").text()).toBe("This PDF is too big to present (32 MB at most)");
    expect(sent("core_read_message_file")).toEqual([]);
  });

  it("waits for a PDF still on its way with a spinner that takes no room", async () => {
    messages = [pdfView("m1", "transferring")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present-loading']").exists()).toBe(true);
    expect(sent("core_read_message_file")).toEqual([]);
    const rule = source.slice(source.indexOf(".ft-call__present-wait {"));
    expect(rule.slice(0, rule.indexOf("}"))).toMatch(/position:\s*absolute/);
  });

  it("asks once before following with a tool not yet allowed; the call stays as it was until then", async () => {
    tools = [tool(BOARD, "Board", false), tool(PDF, "PDF viewer", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    const ask = wrapper.findComponent(GamePermissions);
    expect(ask.props()).toMatchObject({ open: true, name: "Board", body: "Maria López wants to present with Board", allowLabel: "Allow" });
    expect(wrapper.find("[data-test='present-area']").exists()).toBe(false);
    hooks.core_plugin_grant = () => {
      tools = [tool(BOARD, "Board", true), tool(PDF, "PDF viewer", false)];
    };
    ask.vm.$emit("allow");
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("presenting")).toBe("follow");
  });

  it("does not ask again about a presentation the user said no to", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    wrapper.findComponent(GamePermissions).vm.$emit("cancel");
    await flushPromises();
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(false);
    expect(wrapper.find("[data-test='present-area']").exists()).toBe(false);
  });

  // Asked once per presentation, not once per call.
  it("asks again about their next presentation after the user said no to one", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    wrapper.findComponent(GamePermissions).vm.$emit("cancel");
    await flushPromises();
    call.presenting = null;
    await flushPromises();
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    expect(wrapper.findComponent(GamePermissions).props()).toMatchObject({ open: true, body: "Maria López wants to present with Board" });
  });

  it("says the presentation could not start when the permission could not be given", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    wrapper.findComponent(GamePermissions).vm.$emit("allow");
    await flushPromises();
    expect(sent("core_plugin_grant")).toHaveLength(1);
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(false);
    expect(wrapper.find("[data-test='present-failed']").text()).toBe("The presentation couldn't start");
  });

  it("says when this phone lacks the tool, and the call goes on", async () => {
    tools = [];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const missing = await open();
    expect(missing.find("[data-test='present-missing']").text()).toContain("Add it in Apps");
    expect(missing.find("[aria-label='Hang up']").exists()).toBe(true);
  });

  // Product gap found on the phones (2026-10-09, §56): a fresh install offers the board and the
  // PDF viewer to present, and the app carries both, so a missing one is added from what the app
  // carries right there, never with a trip to Apps; only a tool it does not carry is "missing".
  const carried = (id: string, name: string) => ({ id, name, version: "1.1.0", summary: "", size: 1000, installed: false, carried: true });

  it("adds the tool the app carries before presenting with it, then asks for the live channel", async () => {
    active();
    tools = [];
    hooks.core_catalogue = () => [carried(BOARD, "Board"), carried(PDF, "PDF viewer")];
    hooks.core_plugin_add = () => {
      tools = [tool(BOARD, "Board", false)];
    };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(sent("core_plugin_add")).toEqual([{ plugin: BOARD }]);
    expect(toast.create).not.toHaveBeenCalled();
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(true);
  });

  it("says the tool is missing when nothing offers it, and adds nothing", async () => {
    active();
    tools = [];
    hooks.core_catalogue = () => [carried(PDF, "PDF viewer")];
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(sent("core_plugin_add")).toEqual([]);
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("Add it in Apps") }));
  });

  // Fix round 1: once the catalogue has a newer version than the one the app carries, its entry
  // replaces the seed's (`carried: false`); the core still picks seed or download, as in Apps.
  it("adds the tool to present with when the catalogue offers a newer version than the app carries", async () => {
    active();
    tools = [];
    hooks.core_catalogue = () => [{ ...carried(BOARD, "Board"), version: "1.1.1", carried: false }];
    hooks.core_plugin_add = () => {
      tools = [tool(BOARD, "Board", false)];
    };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(sent("core_plugin_add")).toEqual([{ plugin: BOARD }]);
    expect(toast.create).not.toHaveBeenCalled();
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(true);
  });

  it("says presenting failed, not that the tool is missing, when adding it fails", async () => {
    active();
    tools = [];
    hooks.core_catalogue = () => [carried(BOARD, "Board")];
    hooks.core_plugin_add = () => Promise.reject("no room");
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(toast.create).toHaveBeenCalledTimes(1);
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "The presentation couldn't start" }));
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(false);
  });

  // Adding a tool can take seconds (the catalogue, a download): theirs may start meanwhile.
  it("opens no sheet of its own when their presentation started while the tool was being added", async () => {
    active();
    tools = [];
    hooks.core_catalogue = () => [carried(BOARD, "Board")];
    // Both sides may be adding it (mine, then theirs to follow): every add waits for this.
    const adds: Array<() => void> = [];
    const added = () => {
      tools = [tool(BOARD, "Board", false)];
      adds.splice(0).forEach((done) => done());
    };
    hooks.core_plugin_add = () => new Promise<void>((done) => adds.push(done));
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    added();
    await flushPromises();
    expect(actions.presentInCall).not.toHaveBeenCalled();
    expect(toast.create).not.toHaveBeenCalled();
    const ask = wrapper.findComponent(GamePermissions);
    expect(ask.props()).toMatchObject({ open: true, body: "Maria López wants to present with Board" });
  });

  it("adds the tool to follow with when the catalogue offers a newer version than the app carries", async () => {
    tools = [];
    hooks.core_catalogue = () => [{ ...carried(BOARD, "Board"), version: "1.1.1", carried: false }];
    hooks.core_plugin_add = () => {
      tools = [tool(BOARD, "Board", false)];
    };
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(sent("core_plugin_add")).toEqual([{ plugin: BOARD }]);
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(false);
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(true);
  });

  it("says their presentation could not start, not that the tool is missing, when adding it fails", async () => {
    tools = [];
    hooks.core_catalogue = () => [carried(BOARD, "Board")];
    hooks.core_plugin_add = () => Promise.reject("no room");
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(sent("core_plugin_add")).toEqual([{ plugin: BOARD }]);
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(false);
    expect(wrapper.find("[data-test='present-failed']").exists()).toBe(true);
  });

  it("says the tool to follow with is missing when nothing offers it", async () => {
    tools = [];
    hooks.core_catalogue = () => [];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(sent("core_plugin_add")).toEqual([]);
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(true);
  });

  it("adds the tool the app carries to follow their presentation, waiting meanwhile, then asks", async () => {
    tools = [];
    hooks.core_catalogue = () => [carried(BOARD, "Board")];
    let added: () => void = () => undefined;
    hooks.core_plugin_add = () =>
      new Promise<void>((done) => {
        added = () => {
          tools = [tool(BOARD, "Board", false)];
          done();
        };
      });
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(sent("core_plugin_add")).toEqual([{ plugin: BOARD }]);
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(false);
    expect(wrapper.find("[data-test='present-loading']").exists()).toBe(true);
    added();
    await flushPromises();
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(false);
    const ask = wrapper.findComponent(GamePermissions);
    expect(ask.props()).toMatchObject({ open: true, body: "Maria López wants to present with Board" });
  });

  // Ioan, 2026-10-08: watching a presentation is free; the core opens a follower's tool even locked.
  it("follows a presentation with the tools locked, with no lock in sight", async () => {
    planState = "limited";
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(wrapper.findComponent(PluginSheet).props("presenting")).toBe("follow");
    expect(wrapper.text()).not.toContain("Subscribe to use the tools");
  });

  it("says the presentation could not start when the core refuses to open the tool", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    goodbye(wrapper);
    wrapper.findComponent(PluginSheet).vm.$emit("refused");
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
    expect(wrapper.find("[data-test='present-failed']").text()).toBe("The presentation couldn't start");
    expect(wrapper.find("[aria-label='Hang up']").exists()).toBe(true);
  });

  // A lost link said again (plan A): the same presentation keeps its tool as it is.
  it("keeps the tool open when the same presentation is said again", async () => {
    messages = [pdfView("m1", "done")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    const before = wrapper.findComponent(PluginSheet).element;
    call.presenting = { plugin: PDF, file: "m1", by: "them" };
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).element).toBe(before);
    expect(sent("core_read_message_file")).toHaveLength(1);
  });

  it("shows no presentation until the call is connected", async () => {
    active({ phase: "connecting", presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present-area']").exists()).toBe(false);
    expect(wrapper.find("[data-test='presenter']").exists()).toBe(false);
  });

  // A spinner that never ends would say the file is still coming: it is not.
  it("says the presentation could not start when their PDF fails to arrive", async () => {
    messages = [pdfView("m1", "failed")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='present-loading']").exists()).toBe(false);
    expect(sent("core_read_message_file")).toEqual([]);
    expect(wrapper.find("[data-test='present-failed']").text()).toBe("The presentation couldn't start");
  });

  // Nothing moves on screen: the notices and the controls stay where they were.
  it("hides who is called without taking them out of the layout while presenting", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const peer = (await open()).find(".ft-call__peer").element as HTMLElement;
    expect(peer.style.display).not.toBe("none");
    expect(peer.style.visibility).toBe("hidden");
  });

  it("goes back to the call as it was when the presentation stops", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open(true);
    expect(wrapper.find(".ft-call__peer").isVisible()).toBe(false);
    goodbye(wrapper);
    call.presenting = null;
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
    expect(wrapper.find(".ft-call__peer").isVisible()).toBe(true);
  });

  // The tool hears `ft.onClose` and has its say before it goes, as in a chat (ChatThread).
  it("lets the tool say goodbye when the presentation stops, out of sight, and only then takes it down", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open(true);
    const close = goodbye(wrapper, true);
    call.presenting = null;
    await flushPromises();
    expect(close).toHaveBeenCalledTimes(1);
    const sheet = wrapper.findComponent(PluginSheet);
    expect(sheet.exists()).toBe(true);
    expect(wrapper.find("[data-test='present-area']").isVisible()).toBe(false);
    expect(wrapper.find(".ft-call__peer").isVisible()).toBe(true);
    sheet.vm.$emit("closed");
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).exists()).toBe(false);
    expect(wrapper.find("[data-test='present-area']").exists()).toBe(false);
  });

  it("lets the tool say goodbye when the call ends", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    const close = goodbye(wrapper, true);
    call.phase = "ended";
    await flushPromises();
    expect(close).toHaveBeenCalledTimes(1);
  });

  it("opens their next presentation once the last tool has said goodbye", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    const close = goodbye(wrapper, true);
    const first = wrapper.findComponent(PluginSheet);
    call.presenting = { plugin: PDF, by: "them" };
    await flushPromises();
    expect(close).toHaveBeenCalledTimes(1);
    expect(wrapper.findAllComponents(PluginSheet)).toHaveLength(1);
    first.vm.$emit("closed");
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("plugin")).toMatchObject({ id: PDF });
  });

  it("stops presenting when the presenter's tool asks to close; a follower's only closes it here", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const lead = await open();
    goodbye(lead);
    lead.findComponent(PluginSheet).vm.$emit("done");
    expect(actions.stopPresenting).toHaveBeenCalledWith("x");
    actions.stopPresenting.mockReset();
    active({ presenting: { plugin: BOARD, by: "them" } });
    (await open()).findComponent(PluginSheet).vm.$emit("done");
    expect(actions.stopPresenting).not.toHaveBeenCalled();
  });

  // Their picture over a presentation (2026-10-08): a small corner the presentation leaves a hole
  // for, since the native picture sits under the WebView; mine hides meanwhile.
  const lastMeasure = () => actions.layoutVideo.mock.calls.at(-1)?.[0] as () => Record<string, unknown>;
  const place = (element: Element, rect: { x: number; y: number; width: number; height: number }) =>
    (element.getBoundingClientRect = () => ({ ...rect, top: rect.y, left: rect.x, right: rect.x + rect.width, bottom: rect.y + rect.height, toJSON: () => rect }) as DOMRect);

  it("shrinks their picture to a corner while presenting, hides mine, and tells the core", async () => {
    active({ view: view({ remote: true, camera: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    const remote = wrapper.find("[data-test='remote-slot']");
    expect(remote.classes()).toContain("is-thumb");
    expect(wrapper.find("[data-test='local-slot']").exists()).toBe(false);
    place(remote.element, { x: 280, y: 520, width: 96, height: 140 });
    expect(lastMeasure()()).toMatchObject({ remote: { x: 280, y: 520, width: 96, height: 140 }, local: null, localRadius: 0 });
  });

  it("leaves a hole in the presentation where their picture shows through", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    place(wrapper.find("[data-test='present-area']").element, { x: 0, y: 60, width: 400, height: 600 });
    place(wrapper.find("[data-test='remote-slot']").element, { x: 290, y: 500, width: 96, height: 140 });
    window.dispatchEvent(new Event("resize"));
    await nextTick();
    expect(wrapper.find("[data-test='present-area']").attributes("style")).toContain("polygon(evenodd");
  });

  // Defect J (2026-10-09): the presentation ends where the call's buttons start, measured, so
  // nothing of the tool sits under them on any phone.
  it("ends the presentation where the call's buttons start", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    place(wrapper.find(".ft-call__body").element, { x: 0, y: 0, width: 400, height: 800 });
    place(wrapper.find(".ft-call__controls").element, { x: 40, y: 703, width: 320, height: 58 });
    window.dispatchEvent(new Event("resize"));
    await nextTick();
    expect(wrapper.find("[data-test='present-area']").attributes("style")).toContain("bottom: 97px");
  });

  it("drags their picture while presenting", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    const thumb = wrapper.find("[data-test='remote-slot']");
    actions.layoutVideo.mockClear();
    await thumb.trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 1 });
    await thumb.trigger("pointermove", { clientX: 200, clientY: 500, pointerId: 1 });
    await thumb.trigger("pointerup", { clientX: 200, clientY: 500, pointerId: 1 });
    expect(thumb.attributes("style")).toContain("translate(-100px, -100px)");
    expect(actions.layoutVideo).toHaveBeenCalled();
  });

  // Defect I (2026-10-09): on the iPhone the picture moved 7–10 pt and stopped, once the finger
  // left it for the presentation (and its frame) around it. The drag follows the finger anywhere,
  // the frame takes no touch meanwhile, and the capture is let go at the end.
  it("keeps dragging their picture when the finger leaves it for the presentation", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open(true);
    const thumb = wrapper.find("[data-test='remote-slot']");
    const element = thumb.element as HTMLElement;
    const capture = vi.fn();
    const release = vi.fn();
    Object.assign(element, { setPointerCapture: capture, releasePointerCapture: release });
    await thumb.trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 7 });
    expect(capture).toHaveBeenCalledWith(7);
    expect(wrapper.find("[data-test='present-area']").classes()).toContain("is-dragging");
    window.dispatchEvent(new PointerEvent("pointermove", { clientX: 60, clientY: 200, pointerId: 7 }));
    await nextTick();
    expect(thumb.attributes("style")).toContain("translate(-240px, -400px)");
    window.dispatchEvent(new PointerEvent("pointerup", { clientX: 60, clientY: 200, pointerId: 7 }));
    await nextTick();
    expect(release).toHaveBeenCalledWith(7);
    expect(wrapper.find("[data-test='present-area']").classes()).not.toContain("is-dragging");
    expect(source).toMatch(/\.ft-call__present\.is-dragging\s*\{[^}]*pointer-events:\s*none/);
  });

  // The hole follows the picture once it has moved on screen, not where it was a step before.
  it("moves the hole with their picture as it is dragged", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    place(wrapper.find("[data-test='present-area']").element, { x: 0, y: 60, width: 400, height: 600 });
    const thumb = wrapper.find("[data-test='remote-slot']");
    const element = thumb.element as HTMLElement;
    element.getBoundingClientRect = () => {
      const [, dx = "0", dy = "0"] = /translate\((-?\d+)px, (-?\d+)px\)/.exec(element.style.transform) ?? [];
      const rect = { x: 290 + Number(dx), y: 500 + Number(dy), width: 96, height: 140 };
      return { ...rect, top: rect.y, left: rect.x, right: rect.x + rect.width, bottom: rect.y + rect.height, toJSON: () => rect } as DOMRect;
    };
    await thumb.trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 1 });
    await thumb.trigger("pointermove", { clientX: 200, clientY: 500, pointerId: 1 });
    await nextTick();
    expect(wrapper.find("[data-test='present-area']").attributes("style")).toContain("190px 340px, 286px 340px");
  });

  // Both slots take the thumb now: only the small one moves, never from a press on the big one.
  it("moves my thumbnail only when it is the one dragged", async () => {
    active({ view: view({ remote: true, camera: true }) });
    const wrapper = await open();
    const full = wrapper.find("[data-test='remote-slot']");
    await full.trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 1 });
    await full.trigger("pointermove", { clientX: 200, clientY: 500, pointerId: 1 });
    await full.trigger("pointerup", { clientX: 200, clientY: 500, pointerId: 1 });
    expect(wrapper.find("[data-test='local-slot']").attributes("style")).toBeUndefined();
    expect(full.attributes("style")).toBeUndefined();
  });

  it("puts my picture back, full or as a thumbnail, when the presentation stops", async () => {
    active({ view: view({ remote: true, camera: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    goodbye(wrapper);
    actions.layoutVideo.mockClear();
    call.presenting = null;
    await flushPromises();
    expect(wrapper.find("[data-test='local-slot']").classes()).toContain("is-thumb");
    expect(wrapper.find("[data-test='remote-slot']").classes()).not.toContain("is-thumb");
    expect(actions.layoutVideo).toHaveBeenCalled();
    expect(lastMeasure()()).toMatchObject({ local: { x: 0, y: 0, width: 0, height: 0 }, localRadius: 16 });
    expect(wrapper.find("[data-test='present-area']").exists()).toBe(false);
  });

  // Only the board and the PDF viewer present (2026-10-09): a tool that talks to its twin but does
  // not present (a list, a poll, a live game) is never opened, nor asked about, by a presentation.
  it("opens no tool but the two that present, whatever the presentation names", async () => {
    const LIST = "com.flickertalk.list";
    tools = [tool(BOARD, "Board", true), tool(LIST, "List", true)];
    active({ presenting: { plugin: LIST, by: "them" } });
    const granted = await open();
    expect(granted.findComponent(PluginSheet).exists()).toBe(false);
    expect(granted.find("[data-test='present-missing']").exists()).toBe(true);
    tools = [tool(BOARD, "Board", true), tool(LIST, "List", false)];
    const asking = await open();
    expect(asking.findComponent(GamePermissions).props("open")).toBe(false);
    expect(asking.find("[data-test='present-missing']").exists()).toBe(true);
  });

  // A student who said no can change their mind: who presents becomes a button that asks again.
  it("asks again when the user, having said no, taps who presents", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    expect(wrapper.find("[data-test='presenter-ask']").exists()).toBe(false);
    wrapper.findComponent(GamePermissions).vm.$emit("cancel");
    await flushPromises();
    const again = wrapper.find("[data-test='presenter-ask']");
    expect(again.attributes()).toMatchObject({ fill: "clear", "aria-label": "Maria López wants to present with Board" });
    expect(again.text()).toBe("Maria López is presenting");
    await again.trigger("click");
    await flushPromises();
    expect(wrapper.findComponent(GamePermissions).props()).toMatchObject({ open: true, body: "Maria López wants to present with Board" });
  });

  it("says presenting failed when the permission to present could not be given", async () => {
    active();
    tools = [tool(BOARD, "Board", false), tool(PDF, "PDF viewer", false)];
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    wrapper.findComponent(GamePermissions).vm.$emit("allow");
    await flushPromises();
    expect(sent("core_plugin_grant")).toHaveLength(1);
    expect(actions.presentInCall).not.toHaveBeenCalled();
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "The presentation couldn't start" }));
  });

  // 96 px wide: the words do not fit the corner, the icon says it, named for screen readers.
  it("says their camera is paused with the icon alone while their picture is the small corner", async () => {
    active({ view: view({ remote: true, remotePaused: true }), presenting: { plugin: BOARD, by: "me" } });
    const paused = (await open()).find("[data-test='remote-paused']");
    expect(paused.attributes()).toMatchObject({ role: "img", "aria-label": "Camera paused" });
    expect(paused.text()).toBe("");
  });

  // The closing tool can still refuse on its way out: that is not the next presentation failing.
  it("lets a tool on its way out not fail the next presentation", async () => {
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    goodbye(wrapper, true);
    const first = wrapper.findComponent(PluginSheet);
    call.presenting = { plugin: PDF, by: "them" };
    await flushPromises();
    first.vm.$emit("refused");
    first.vm.$emit("closed");
    await flushPromises();
    expect(wrapper.find("[data-test='present-failed']").exists()).toBe(false);
    expect(wrapper.findComponent(PluginSheet).props("plugin")).toMatchObject({ id: PDF });
  });

  // Their picture went mid-drag (their camera off): when it comes back, a move of that finger is no drag.
  it("forgets a drag cut short when the small picture goes", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    await wrapper.find("[data-test='remote-slot']").trigger("pointerdown", { clientX: 300, clientY: 600, pointerId: 1 });
    call.view = view();
    await flushPromises();
    call.view = view({ remote: true });
    await flushPromises();
    const thumb = wrapper.find("[data-test='remote-slot']");
    expect(thumb.classes()).toContain("is-thumb");
    await thumb.trigger("pointermove", { clientX: 200, clientY: 500, pointerId: 1 });
    expect(thumb.attributes("style")).toBeUndefined();
  });

  it("says their presentation could not start when the plugins could not be read after the permission", async () => {
    tools = [tool(BOARD, "Board", false)];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = await open();
    reading.fails = true;
    wrapper.findComponent(GamePermissions).vm.$emit("allow");
    await flushPromises();
    reading.fails = false;
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(false);
    expect(wrapper.find("[data-test='present-failed']").text()).toBe("The presentation couldn't start");
  });

  it("sends no PDF picked after the call ended", async () => {
    active();
    hooks.core_pick_files = () => {
      call.phase = "ended";
      return [picked];
    };
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[1].handler?.();
    await flushPromises();
    expect(sent("core_send_picked")).toEqual([]);
    expect(actions.presentInCall).not.toHaveBeenCalled();
  });

  it("closes the sheets of presenting when the call ends", async () => {
    active();
    tools = [tool(BOARD, "Board", false), tool(PDF, "PDF viewer", false)];
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    expect(wrapper.findComponent(IonActionSheet).props("isOpen")).toBe(true);
    call.phase = "ended";
    await flushPromises();
    expect(wrapper.findComponent(IonActionSheet).props("isOpen")).toBe(false);
    call.phase = "active";
    await flushPromises();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(true);
    call.phase = "ended";
    await flushPromises();
    expect(wrapper.findComponent(GamePermissions).props("open")).toBe(false);
  });

  // Defect H (2026-10-09): both pressed Present at once. The loser's screen follows its core: no
  // sheet of its own left open, its own tool handed over to the winner's, no Stop.
  it("closes Present's sheet when the other side starts presenting", async () => {
    active();
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    expect(wrapper.findComponent(IonActionSheet).props("isOpen")).toBe(true);
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    expect(wrapper.findComponent(IonActionSheet).props("isOpen")).toBe(false);
  });

  it("closes my own permission sheet when the other side starts presenting, and asks about theirs", async () => {
    active();
    tools = [tool(BOARD, "Board", false), tool(PDF, "PDF viewer", false)];
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    const ask = wrapper.findComponent(GamePermissions);
    expect(ask.props("body")).toBe("Talk to the same plugin on the other side of the chat");
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    expect(ask.props("open")).toBe(true);
    expect(ask.props("body")).toBe("Maria López wants to present with Board");
    ask.vm.$emit("allow");
    await flushPromises();
    expect(actions.presentInCall).not.toHaveBeenCalled();
  });

  it("hands its own tool over to theirs when the core says they won", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    const wrapper = await open();
    expect(wrapper.findComponent(PluginSheet).props("presenting")).toBe("lead");
    const close = goodbye(wrapper, true);
    const mine = wrapper.findComponent(PluginSheet);
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    expect(close).toHaveBeenCalledTimes(1);
    expect(wrapper.find("[data-test='stop-presenting']").exists()).toBe(false);
    expect(wrapper.find("[data-test='present']").exists()).toBe(false);
    mine.vm.$emit("closed");
    await flushPromises();
    const theirs = wrapper.findComponent(PluginSheet);
    expect(theirs.props()).toMatchObject({ plugin: { id: BOARD }, presenting: "follow" });
    expect(wrapper.find("[data-test='presenter']").exists()).toBe(true);
  });

  it("presents nothing, and says nothing, when their presentation came while mine was on its way", async () => {
    active();
    let refuse: (error: Error) => void = () => undefined;
    actions.presentInCall.mockImplementationOnce(() => new Promise((_, reject) => (refuse = reject)));
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    (wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>)[0].handler?.();
    await flushPromises();
    call.presenting = { plugin: BOARD, by: "them" };
    refuse(new Error("the other side is presenting"));
    await flushPromises();
    expect(toast.create).not.toHaveBeenCalled();
  });

  it("does not present from a sheet tapped after the other side started", async () => {
    active();
    const wrapper = await open();
    await wrapper.find("[data-test='present']").trigger("click");
    const buttons = wrapper.findComponent(IonActionSheet).props("buttons") as Array<{ handler?: () => void }>;
    call.presenting = { plugin: BOARD, by: "them" };
    await flushPromises();
    buttons[0].handler?.();
    await flushPromises();
    expect(actions.presentInCall).not.toHaveBeenCalled();
  });

  it("says so, and rejects nothing unheard, when the presenter's tool closes and stopping fails", async () => {
    active({ presenting: { plugin: BOARD, by: "me" } });
    actions.stopPresenting.mockRejectedValueOnce(new Error("busy"));
    const wrapper = await open();
    goodbye(wrapper);
    wrapper.findComponent(PluginSheet).vm.$emit("done");
    await flushPromises();
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message: "The presentation couldn't start" }));
  });

  it("does not cover the presentation with the invitation to turn the camera on", async () => {
    active({ view: view({ remote: true }), presenting: { plugin: BOARD, by: "me" } });
    expect((await open()).text()).not.toContain("Turn on your camera");
  });

  it("tells screen readers what the presentation waits for", async () => {
    messages = [pdfView("m1", "transferring")];
    active({ presenting: { plugin: PDF, file: "m1", by: "them" } });
    const spinner = (await open()).find("[data-test='present-loading']");
    expect(spinner.attributes()).toMatchObject({ role: "status", "aria-label": "Maria López is presenting" });
    expect(spinner.attributes("aria-hidden")).toBeUndefined();
  });

  // Opened cold (from CallKit, say), the list of plugins is not read yet: no "missing" flashes.
  it("waits, rather than say the tool is missing, until this phone's plugins are read", async () => {
    const answers: Array<(value: unknown) => void> = [];
    hooks.core_plugins = () => new Promise((done) => answers.push(done));
    installed.value = [];
    active({ presenting: { plugin: BOARD, by: "them" } });
    const wrapper = mount(CallPage, { shallow: true });
    await flushPromises();
    expect(wrapper.find("[data-test='present-missing']").exists()).toBe(false);
    expect(wrapper.find("[data-test='present-loading']").exists()).toBe(true);
    for (const answer of answers) answer(tools);
    await flushPromises();
    expect(wrapper.findComponent(PluginSheet).props("presenting")).toBe("follow");
  });
});
