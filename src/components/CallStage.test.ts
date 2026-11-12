import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { call, resetCalls } from "../__tests__/calls-mock";
import CallStage from "./CallStage.vue";

vi.mock("../calls", async () => (await import("../__tests__/calls-mock")).callsMock());

type StageProps = {
  native: boolean;
  isVideo: boolean;
  stage: boolean;
  showRemote: boolean;
  showLocal: boolean;
  remoteThumb: boolean;
  localThumb: boolean;
  thumbStyle?: { transform: string };
};
const stage = (props: Partial<StageProps> = {}) =>
  mount(CallStage, {
    shallow: true,
    props: { native: false, isVideo: false, stage: false, showRemote: false, showLocal: false, remoteThumb: false, localThumb: false, ...props },
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

describe("CallStage", () => {
  beforeEach(() => {
    resetCalls();
    Object.assign(call, { id: "x", contact: "c1", phase: "active", since: Date.now() });
  });

  it("on a phone, leaves a see-through hole for each native picture, the small one moved where it was dragged", () => {
    const wrapper = stage({ native: true, stage: true, showRemote: true, showLocal: true, localThumb: true, thumbStyle: { transform: "translate(-10px, -20px)" } });
    expect(wrapper.find(".ft-call__stage").attributes("data-test")).toBe("video");
    expect(wrapper.find("[data-test='remote-slot']").classes()).not.toContain("is-thumb");
    expect(wrapper.find("[data-test='remote-slot']").attributes("style")).toBeUndefined();
    const local = wrapper.find("[data-test='local-slot']");
    expect(local.classes()).toContain("is-thumb");
    expect(local.attributes("style")).toContain("translate(-10px, -20px)");
    expect(wrapper.find("video").exists()).toBe(false);
  });

  it("on a phone, keeps an empty stage until there is a picture", () => {
    const wrapper = stage({ native: true });
    expect(wrapper.find(".ft-call__stage").attributes("data-test")).toBeUndefined();
    expect(wrapper.find("[data-test='remote-slot']").exists()).toBe(false);
    expect(wrapper.find("[data-test='local-slot']").exists()).toBe(false);
  });

  it("says their camera is paused, with the icon alone while their picture is the small one", async () => {
    call.view = view({ remote: true, remotePaused: true });
    const full = stage({ native: true, stage: true, showRemote: true });
    expect(full.find("[data-test='remote-paused']").text()).toContain("Camera paused");
    await full.setProps({ remoteThumb: true });
    expect(full.find("[data-test='remote-paused']").attributes()).toMatchObject({ role: "img", "aria-label": "Camera paused" });
    expect(full.find("[data-test='remote-paused']").text()).toBe("");
  });

  it("marks my picture as paused", () => {
    call.view = view({ camera: true, paused: true });
    const wrapper = stage({ native: true, stage: true, showLocal: true });
    expect(wrapper.find("[data-test='local-slot'] [aria-label='Camera paused']").exists()).toBe(true);
  });

  it("hands the pointer on a picture to the page", async () => {
    const wrapper = stage({ native: true, stage: true, showRemote: true, showLocal: true, localThumb: true });
    const local = wrapper.find("[data-test='local-slot']");
    await local.trigger("pointerdown", { pointerId: 1 });
    await local.trigger("pointermove", { pointerId: 1 });
    await local.trigger("pointerup", { pointerId: 1 });
    await wrapper.find("[data-test='remote-slot']").trigger("pointercancel", { pointerId: 2 });
    expect(wrapper.emitted("grab")).toHaveLength(1);
    expect(wrapper.emitted("move")).toHaveLength(1);
    expect(wrapper.emitted("letGo")).toHaveLength(2);
    expect((wrapper.emitted("grab")![0][0] as PointerEvent).type).toBe("pointerdown");
  });

  it("shows where the native pictures go, for the page to measure", async () => {
    const wrapper = stage({ native: true, stage: true, showRemote: true, showLocal: true, localThumb: true });
    const exposed = wrapper.vm as unknown as { remoteSlot: Element | null; localSlot: Element | null };
    expect(exposed.remoteSlot).toBe(wrapper.find("[data-test='remote-slot']").element);
    expect(exposed.localSlot).toBe(wrapper.find("[data-test='local-slot']").element);
    await wrapper.setProps({ showRemote: false });
    expect(exposed.remoteSlot).toBeNull();
  });

  it("on the desktop, shows the contact's video only once it arrives", async () => {
    const wrapper = stage({ isVideo: true });
    expect(wrapper.find("[data-test='video']").exists()).toBe(true);
    expect(wrapper.find(".ft-call__remote").exists()).toBe(false);
    call.remote = new MediaStream();
    await nextTick();
    const exposed = wrapper.vm as unknown as { remoteVideo: Element | null; localVideo: Element | null };
    expect(exposed.remoteVideo).toBe(wrapper.find(".ft-call__remote").element);
    expect(exposed.localVideo).toBe(wrapper.find(".ft-call__self").element);
  });

  it("on the desktop, plays a voice call through an audio element", () => {
    const wrapper = stage();
    expect(wrapper.find("[data-test='video']").exists()).toBe(false);
    expect((wrapper.vm as unknown as { remoteAudio: Element | null }).remoteAudio).toBe(wrapper.find("audio").element);
  });
});
