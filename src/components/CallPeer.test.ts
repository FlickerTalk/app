import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { call, resetCalls } from "../__tests__/calls-mock";
import CallPeer from "./CallPeer.vue";

vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

const peer = (avatar = true) => mount(CallPeer, { shallow: true, props: { name: "Maria López", hue: 200, avatar } });

describe("CallPeer", () => {
  beforeEach(() => {
    resetCalls();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the name in its own direction", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const name = peer().find(".ft-call__name");
    expect(name.text()).toBe("Maria López");
    expect(name.attributes("dir")).toBe("auto");
  });

  it("says the call is being placed, then connecting", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    const wrapper = peer();
    expect(wrapper.find(".ft-call__state").text()).toBe("Calling…");
    call.phase = "connecting";
    await nextTick();
    expect(wrapper.find(".ft-call__state").text()).toBe("Connecting…");
  });

  it("shows the call's clock once it is connected, and keeps it going", async () => {
    vi.useFakeTimers();
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() - 65_000 });
    const wrapper = peer();
    const state = wrapper.find(".ft-call__state");
    expect(state.text()).toBe("01:05");
    expect(state.classes()).toContain("is-live");
    vi.advanceTimersByTime(2000);
    await nextTick();
    expect(state.text()).toBe("01:07");
  });

  it("says how the call ended", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ended", outcome: "busy" });
    const state = peer().find(".ft-call__state");
    expect(state.text()).toBe("Busy");
    expect(state.classes()).not.toContain("is-live");
  });

  it("shows the contact's picture only when there is no video to show", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "calling" });
    expect(peer(true).findComponent({ name: "Avatar" }).props()).toMatchObject({ name: "Maria López", hue: 200, size: 132 });
    expect(peer(false).findComponent({ name: "Avatar" }).exists()).toBe(false);
  });
});
