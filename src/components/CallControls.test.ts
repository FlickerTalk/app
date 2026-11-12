import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount, type VueWrapper } from "@vue/test-utils";
import { nextTick } from "vue";
import { IonIcon } from "@ionic/vue";
import { headsetOutline, micOffOutline, phonePortraitOutline, volumeHighOutline } from "ionicons/icons";
import { actions, call, resetCalls } from "../__tests__/calls-mock";
import CallControls from "./CallControls.vue";

vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

const controls = (props: Partial<{ native: boolean; isVideo: boolean; invite: boolean; cameraUsable: boolean; cameraRunning: boolean }> = {}) =>
  mount(CallControls, {
    shallow: true,
    props: { native: false, isVideo: false, invite: false, cameraUsable: false, cameraRunning: false, ...props },
  });
const button = (wrapper: VueWrapper, label: string) => wrapper.find(`[aria-label='${label}']`);

describe("CallControls", () => {
  beforeEach(() => {
    resetCalls();
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
  });

  it("is one row of the call's buttons", () => {
    expect(controls().classes()).toContain("ft-call__controls");
  });

  it("mutes and switches the speaker through the call", async () => {
    call.muted = true;
    const wrapper = controls();
    expect(button(wrapper, "Mute").attributes("aria-pressed")).toBe("true");
    expect(button(wrapper, "Mute").findComponent(IonIcon).props("icon")).toBe(micOffOutline);
    await button(wrapper, "Mute").trigger("click");
    await button(wrapper, "Speaker").trigger("click");
    expect(actions.toggleMute).toHaveBeenCalled();
    expect(actions.toggleSpeaker).toHaveBeenCalled();
  });

  it("shows where the voice goes on the speaker button", async () => {
    Object.assign(call, { speaker: false, route: "headset" });
    const wrapper = controls();
    const icon = () => button(wrapper, "Speaker").findComponent(IonIcon).props("icon");
    expect(icon()).toBe(headsetOutline);
    Object.assign(call, { speaker: true, route: "speaker" });
    await nextTick();
    expect(icon()).toBe(volumeHighOutline);
    Object.assign(call, { speaker: false, route: "receiver" });
    await nextTick();
    expect(icon()).toBe(phonePortraitOutline);
  });

  it("on a phone, leaves the camera and its flip to the page, and shows them waiting until usable", async () => {
    const wrapper = controls({ native: true });
    const camera = button(wrapper, "Camera");
    const flip = button(wrapper, "Switch camera");
    expect(camera.attributes("aria-disabled")).toBe("true");
    expect(camera.classes()).toContain("is-waiting");
    expect(flip.classes()).toContain("is-waiting");
    await camera.trigger("click");
    await flip.trigger("click");
    expect(wrapper.emitted("camera")).toHaveLength(1);
    expect(wrapper.emitted("flip")).toHaveLength(1);
    expect(actions.toggleCamera).not.toHaveBeenCalled();
    await wrapper.setProps({ cameraUsable: true, cameraRunning: true, invite: true });
    expect(button(wrapper, "Camera").classes()).toEqual(expect.arrayContaining(["is-invite"]));
    expect(button(wrapper, "Camera").classes()).not.toContain("is-waiting");
    expect(button(wrapper, "Switch camera").attributes("aria-disabled")).toBe("false");
  });

  it("on the desktop, turns the camera of a video call off itself and has no flip", async () => {
    const wrapper = controls({ isVideo: true });
    expect(button(wrapper, "Switch camera").exists()).toBe(false);
    await button(wrapper, "Camera").trigger("click");
    expect(actions.toggleCamera).toHaveBeenCalled();
    expect(controls().find("[aria-label='Camera']").exists()).toBe(false);
  });

  it("leaves hanging up to the page", async () => {
    const wrapper = controls();
    await button(wrapper, "Hang up").trigger("click");
    expect(wrapper.emitted("hangUp")).toHaveLength(1);
    expect(actions.hangUp).not.toHaveBeenCalled();
  });
});
