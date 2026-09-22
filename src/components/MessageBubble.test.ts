import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

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

  // A message written with fences is code: monospace, as it was written, with its language.
  it("shows a fenced message as a block of code", () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, text: "```python\nif x:\n    go()\n```" } },
      shallow: true,
    });
    const code = wrapper.find("[data-test='code']");
    expect(code.exists()).toBe(true);
    expect(code.text()).toContain("if x:");
    expect(code.text()).toContain("    go()");
    expect(wrapper.text()).toContain("python");
    expect(wrapper.find("[data-test='code'] script").exists()).toBe(false);
  });

  it("leaves a message that is not code as it is", () => {
    const wrapper = mount(MessageBubble, { props: { message: { ...base, text: "just text" } }, shallow: true });
    expect(wrapper.find("[data-test='code']").exists()).toBe(false);
    expect(wrapper.text()).toContain("just text");
  });

  // A video that arrived plays in the chat, like an image or a voice message.
  it("plays a received video", () => {
    const message = {
      ...base,
      kind: "file",
      file: { name: "clip.mp4", size: "8 MB", mime: "video/mp4", state: "ready", progress: 1, url: "asset://localhost/files/clip.mp4" },
    };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    const video = wrapper.find("video");
    expect(video.exists()).toBe(true);
    expect(video.attributes("src")).toBe("asset://localhost/files/clip.mp4");
    expect(video.attributes("controls")).toBeDefined();
  });

  // Addresses are marked so they can be opened, and nothing is ever fetched to preview them.
  it("marks the links of a message", async () => {
    const wrapper = mount(MessageBubble, {
      props: { message: { ...base, text: "mira https://flickertalk.com y escribe a info@flickertalk.com" } },
      shallow: true,
    });
    const links = wrapper.findAll("[data-test='link']");
    expect(links.map((link) => link.text())).toEqual(["https://flickertalk.com", "info@flickertalk.com"]);
    await links[1].trigger("click");
    expect(opener.openUrl).toHaveBeenCalledWith("mailto:info@flickertalk.com");
    expect(wrapper.find("img[src^='http']").exists()).toBe(false);
  });

  // §53: a message reaches a plugin only when the user hands it over, with a long press.
  it("hands the message to a plugin after a long press, not a tap", async () => {
    vi.useFakeTimers();
    const wrapper = mount(MessageBubble, { props: { message: base, withPlugin: true }, shallow: true });
    const bubble = wrapper.find("[data-test='bubble']");

    await bubble.trigger("pointerdown");
    await bubble.trigger("pointerup");
    vi.advanceTimersByTime(1000);
    expect(wrapper.emitted("plugin")).toBeUndefined();

    await bubble.trigger("pointerdown");
    vi.advanceTimersByTime(600);
    expect(wrapper.emitted("plugin")).toEqual([["m1"]]);
    vi.useRealTimers();
  });

  it("does nothing on a long press when no plugin may read it", async () => {
    vi.useFakeTimers();
    const wrapper = mount(MessageBubble, { props: { message: base }, shallow: true });
    await wrapper.find("[data-test='bubble']").trigger("pointerdown");
    vi.advanceTimersByTime(1000);
    expect(wrapper.emitted("plugin")).toBeUndefined();
    vi.useRealTimers();
  });
});
