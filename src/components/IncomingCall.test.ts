import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import IncomingCall from "./IncomingCall.vue";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

describe("IncomingCall", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    push.mockReset();
  });

  it("stays hidden without a ringing call", () => {
    expect(mount(IncomingCall, { shallow: true }).find("[data-test='incoming']").exists()).toBe(false);
  });

  it("shows who is calling and how", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ringing", video: true });
    const wrapper = mount(IncomingCall, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    expect(wrapper.find("[aria-label='Video']").exists()).toBe(true);
  });

  it("answers and opens the call", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ringing" });
    const wrapper = mount(IncomingCall, { shallow: true });
    await wrapper.find("[aria-label='Answer']").trigger("click");
    await flushPromises();
    expect(actions.acceptCall).toHaveBeenCalled();
    expect(push).toHaveBeenCalledWith("/call/c1");
  });

  it("declines", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ringing" });
    await mount(IncomingCall, { shallow: true }).find("[aria-label='Decline']").trigger("click");
    expect(actions.hangUp).toHaveBeenCalled();
    expect(push).not.toHaveBeenCalled();
  });
});
