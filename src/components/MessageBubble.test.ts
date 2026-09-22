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

  it("says when a transfer failed", () => {
    const message = { ...base, mine: false, kind: "file", file: { name: "a.bin", size: "10 B", progress: 0, state: "failed" } };
    const wrapper = mount(MessageBubble, { props: { message }, shallow: true });
    expect(wrapper.text()).toContain("Failed");
    expect(wrapper.find("[role='progressbar']").exists()).toBe(false);
  });
});
