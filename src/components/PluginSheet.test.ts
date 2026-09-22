import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import PluginSheet from "./PluginSheet.vue";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

describe("PluginSheet", () => {
  beforeEach(() => vi.restoreAllMocks());

  it("shows the plugin in a frame of its own, locked down", async () => {
    const wrapper = mount(PluginSheet, {
      props: { plugin: { id: "com.flickertalk.code", name: "Code block" }, text: "```rust\nfn main() {}\n```" },
      shallow: true,
    });
    await flushPromises();
    const frame = wrapper.find("iframe");
    expect(frame.attributes("src")).toBe("http://ftplugin.localhost/com.flickertalk.code/frame.html");
    // The frame may run its script and nothing else: no forms, no popups, no same-origin.
    expect(frame.attributes("sandbox")).toBe("allow-scripts");
    expect(wrapper.text()).toContain("Code block");
  });

  // §53: the plugin gets the text only when it says it is ready, and nothing else of the chat.
  it("hands the text to the plugin once it is ready", async () => {
    const wrapper = mount(PluginSheet, {
      props: { plugin: { id: "com.flickertalk.code", name: "Code block" }, text: "hello" },
      shallow: true,
    });
    await flushPromises();
    const frame = wrapper.find("iframe").element as HTMLIFrameElement;
    const post = vi.fn();
    Object.defineProperty(frame, "contentWindow", { value: { postMessage: post }, configurable: true });

    window.dispatchEvent(new MessageEvent("message", { data: { type: "ft.ready" }, source: frame.contentWindow }));
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.render", text: "hello" }, "*");
  });
});
