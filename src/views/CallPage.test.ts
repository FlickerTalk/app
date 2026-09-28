import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon } from "@ionic/vue";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallPage from "./CallPage.vue";

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
});
