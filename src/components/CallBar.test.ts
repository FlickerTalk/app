import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallBar from "./CallBar.vue";

const push = vi.fn();
const route = { path: "/tabs/chats" };
vi.mock("vue-router", () => ({ useRouter: () => ({ push }), useRoute: () => route }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

// 2026-09-29: a call going on while the app shows something else can always be gone back to and
// hung up (on the iPhone, a call answered from CallKit's banner left no way to hang up).
describe("CallBar", () => {
  beforeEach(() => {
    seed();
    resetCalls();
    push.mockReset();
    route.path = "/tabs/chats";
  });

  const bar = (wrapper: ReturnType<typeof mount>) => wrapper.find("[data-test='call-bar']");

  it("stays hidden without a call", () => {
    expect(bar(mount(CallBar, { shallow: true })).exists()).toBe(false);
  });

  it("stays hidden while a call rings: the incoming call has its own banner", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "ringing" });
    expect(bar(mount(CallBar, { shallow: true })).exists()).toBe(false);
  });

  it("stays hidden on the call's own screen", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    route.path = "/call/c1";
    expect(bar(mount(CallBar, { shallow: true })).exists()).toBe(false);
  });

  it("goes back to the call going on", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallBar, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    await wrapper.find("[aria-label='Back to the call']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/c1");
  });

  it("shows a call that is still connecting or calling, too", () => {
    for (const phase of ["calling", "connecting"] as const) {
      Object.assign(call, { id: "x", contact: "c1", phase });
      expect(bar(mount(CallBar, { shallow: true })).exists()).toBe(true);
    }
  });

  it("hangs up from anywhere", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    await mount(CallBar, { shallow: true }).find("[aria-label='Hang up']").trigger("click");
    expect(actions.hangUp).toHaveBeenCalled();
    expect(push).not.toHaveBeenCalled();
  });
});
