import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon } from "@ionic/vue";
import { callOutline, pauseCircleOutline, videocamOutline } from "ionicons/icons";
import { seed } from "../__tests__/seed";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallBar from "./CallBar.vue";
import source from "./CallBar.vue?raw";
import base from "../theme/base.css?raw";

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

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
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

  // Native video (2026-09-29): the bar says whether the call has pictures now, not how it began.
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

    // Out of the call screen my camera is held (docs/video-nativo.md): the bar says so.
    it("says my camera is paused while I am elsewhere", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: true, video: true, view: view({ camera: true, paused: true }) });
      const wrapper = mount(CallBar, { shallow: true });
      expect(wrapper.find("[aria-label='Back to the call']").findComponent(IonIcon).props("icon")).toBe(pauseCircleOutline);
      // The button's own label hides what is inside it: the pause is its description.
      const back = wrapper.find("[aria-label='Back to the call']");
      expect(wrapper.find(`#${back.attributes("aria-describedby")}`).text()).toBe("Camera paused");
    });

    it("keeps the call's kind on the desktop", () => {
      Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now(), native: false, video: true });
      expect(icon()).toBe(videocamOutline);
    });
  });

  // Seen on the iPhone (QA, 2026-10-03): over the chat's header the pill cut the contact's name and
  // status, and just below it, it covered the game room's bar. On a phone the bar has a band of its
  // own above the header: while it shows, the page gives it 44 px more of the top inset, so every
  // header moves down. On a wide screen it stays in the header's empty middle.
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
