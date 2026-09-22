import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import MessageBubble from "./MessageBubble.vue";

const base = { id: "m1", text: "Hi", time: "10:02", mine: true };

describe("MessageBubble", () => {
  it.each([
    ["pending", "Waiting for device"],
    ["sent", "Sent"],
    ["delivered", "Delivered"],
    ["read", "Read"],
  ])("labels the %s state for screen readers", (status, label) => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, status } }, shallow: true });
    expect(wrapper.find(`[aria-label="${label}"]`).exists()).toBe(true);
  });

  it("shows no delivery state on incoming messages", () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, mine: false, status: "read" } },
      shallow: true,
    });
    expect(wrapper.find("[aria-label='Read']").exists()).toBe(false);
  });

  it("shows the transfer progress of a file", () => {
    const message = {
      ...base,
      kind: "file",
      status: "sent",
      file: { name: "photos.zip", size: "48 MB", progress: 0.62, state: "sending" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("photos.zip");
    expect(wrapper.find("[role='progressbar']").attributes("aria-valuenow")).toBe("62");
  });

  it("shows a received image", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "beach.jpg", size: "1.2 MB", progress: 1, state: "done", mime: "image/jpeg", url: "asset://localhost/beach.jpg" },
    };
    const image = mount(MessageBubble, { props: { message }, shallow: true }).find("img");
    expect(image.attributes("src")).toBe("asset://localhost/beach.jpg");
    expect(image.attributes("alt")).toBe("beach.jpg");
  });

  it("plays a voice message", () => {
    const message = {
      ...base,
      mine: false,
      kind: "file",
      file: { name: "voice-20260922-161500.m4a", size: "24 KB", progress: 1, state: "done", mime: "audio/mp4", url: "asset://localhost/v.m4a" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.find("audio").attributes("src")).toBe("asset://localhost/v.m4a");
    expect(wrapper.text()).toContain("Voice message");
    expect(wrapper.text()).not.toContain("voice-20260922");
  });

  it("says when a transfer failed", () => {
    const message = { ...base, mine: false, kind: "file", file: { name: "a.bin", size: "10 B", progress: 0, state: "failed" } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("Failed");
    expect(wrapper.find("[role='progressbar']").exists()).toBe(false);
  });

  const arrived = {
    ...base,
    mine: false,
    kind: "file",
    file: { name: "menu.pdf", size: "1.2 MB", progress: 1, state: "done", mime: "application/pdf" },
  };

  it("opens an arrived file when tapped", async () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived }, shallow: true });
    await wrapper.find("[data-test='file']").trigger("click");
    expect(wrapper.emitted("open")).toEqual([["m1"]]);
  });

  it("offers to save an arrived file to Downloads", async () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived }, shallow: true });
    await wrapper.find("[aria-label='Save to Downloads']").trigger("click");
    expect(wrapper.emitted("save")).toEqual([["m1"]]);
    expect(wrapper.emitted("open")).toBeUndefined();
  });

  it("shows when the file has been saved", () => {
    const wrapper = mount(MessageBubble, { props: { message: arrived, saved: true }, shallow: true });
    expect(wrapper.find("[aria-label='Saved to Downloads']").exists()).toBe(true);
  });

  it("cannot open or save a file still on its way", async () => {
    const message = { ...arrived, file: { ...arrived.file, progress: 0.4, state: "receiving" } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    await wrapper.find("[data-test='file']").trigger("click");
    expect(wrapper.emitted("open")).toBeUndefined();
    expect(wrapper.find("[aria-label='Save to Downloads']").exists()).toBe(false);
  });
});
