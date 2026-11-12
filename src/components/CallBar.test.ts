import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon } from "@ionic/vue";
import { callOutline, pauseCircleOutline, videocamOutline } from "ionicons/icons";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import { callScreenMounted } from "../call-screen";
import CallBar from "./CallBar.vue";
import source from "./CallBar.vue?raw";
import base from "../theme/base.css?raw";

const push = vi.fn();
const route = { path: "/tabs/chats" };
vi.mock("vue-router", () => ({ useRouter: () => ({ push, currentRoute: { value: route } }), useRoute: () => route }));
vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

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

  it("shows the name in its own direction", () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    expect(mount(CallBar, { shallow: true }).find(".ft-callbar__name").attributes("dir")).toBe("auto");
  });

  it("goes back to the call going on", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const wrapper = mount(CallBar, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    await wrapper.find("[aria-label='Back to the call']").trigger("click");
    expect(push).toHaveBeenCalledWith("/call/c1");
  });

  it("goes back to the call only once the call screen that was leaving has gone", async () => {
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
    const unmounted = callScreenMounted();
    const wrapper = mount(CallBar, { shallow: true });
    await wrapper.find("[aria-label='Back to the call']").trigger("click");
    await nextTick();
    expect(push).not.toHaveBeenCalled();
    unmounted();
    await vi.waitFor(() => expect(push).toHaveBeenCalledWith("/call/c1"));
    expect(push).toHaveBeenCalledTimes(1);
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

  describe("video state", () => {
    const icon = () => mount(CallBar, { shallow: true }).find("[aria-label='Back to the call']").findComponent(IonIcon).props("icon");
    const view = (patch: Partial<typeof call.view>) => ({
      available: true,
      camera: false,
      paused: false,
      facing: "front" as const,
      remote: false,
      remotePaused: false,
      ...patch,
    });

    it("shows a voice call that went to video", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: false, view: view({ remote: true }) });
      expect(icon()).toBe(videocamOutline);
    });

    it("shows a video call that went back to voice", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: true, view: view({}) });
      expect(icon()).toBe(callOutline);
    });

    it("says my camera is paused while I am elsewhere", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: true, view: view({ camera: true, paused: true }) });
      const wrapper = mount(CallBar, { shallow: true });
      expect(wrapper.find("[aria-label='Back to the call']").findComponent(IonIcon).props("icon")).toBe(pauseCircleOutline);
      const back = wrapper.find("[aria-label='Back to the call']");
      expect(wrapper.find(`#${back.attributes("aria-describedby")}`).text()).toBe("Camera paused");
    });

    it("keeps the call's kind on the desktop", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: false, video: true });
      expect(icon()).toBe(videocamOutline);
    });
  });

  describe("its own band on a phone", () => {
    const band = () => document.documentElement.classList.contains("ft-call-bar");
    beforeEach(() => document.documentElement.classList.remove("ft-call-bar"));

    it("takes the band while it shows", async () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
      const wrapper = mount(CallBar, { shallow: true });
      expect(band()).toBe(true);
      call.phase = "ended";
      await nextTick();
      expect(band()).toBe(false);
      wrapper.unmount();
    });

    it("leaves no band on the call's own screen", () => {
      route.path = "/call/c1";
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
      const wrapper = mount(CallBar, { shallow: true });
      expect(band()).toBe(false);
      wrapper.unmount();
    });

    it("gives the band back when it goes", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
      mount(CallBar, { shallow: true }).unmount();
      expect(band()).toBe(false);
    });

    it("sits in the band on a phone, and in the header on a wide screen", () => {
      const styles = source.slice(source.indexOf("<style"));
      expect(styles).toMatch(/\.ft-callbar\s*{[^}]*top:\s*calc\(env\(safe-area-inset-top\) \+ 6px\)/);
      expect(styles).toMatch(/@media \(max-width: 767px\)\s*{\s*\.ft-callbar\s*{[^}]*top:\s*calc\(env\(safe-area-inset-top\) \+ 4px\)/);
      expect(styles).not.toMatch(/\+ 60px/);
      expect(base).toMatch(
        /@media \(max-width: 767px\)\s*{\s*html\.ft-call-bar\s*{\s*--ion-safe-area-top:\s*calc\(env\(safe-area-inset-top\) \+ 44px\);/,
      );
    });
  });
});
