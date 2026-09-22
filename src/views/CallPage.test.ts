import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallPage from "./CallPage.vue";

const route = { params: { id: "c1" }, query: {} as Record<string, string> };
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => ({ back: vi.fn() }) }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

describe("CallPage", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    route.query = {};
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

  it("shows the video area only on video calls", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling", video: false });
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(false);
    call.video = true;
    expect(mount(CallPage, { shallow: true }).find("[data-test='video']").exists()).toBe(true);
  });
});
