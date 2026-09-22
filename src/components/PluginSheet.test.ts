import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

import PluginSheet from "./PluginSheet.vue";

const plugin = { id: "com.flickertalk.markdown", name: "Markdown" };

/** The frame of the plugin, and what it says to the app. */
function framed(wrapper: ReturnType<typeof mount>) {
  const frame = wrapper.find("iframe").element as HTMLIFrameElement;
  const post = vi.fn();
  Object.defineProperty(frame, "contentWindow", { value: { postMessage: post }, configurable: true });
  const says = (data: unknown) =>
    window.dispatchEvent(new MessageEvent("message", { data, source: frame.contentWindow }));
  return { post, says };
}

describe("PluginSheet", () => {
  beforeEach(() => tauri.invoke.mockReset());

  it("shows the plugin in a frame of its own, locked down", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const frame = wrapper.find("iframe");
    expect(frame.attributes("src")).toBe("http://ftplugin.localhost/com.flickertalk.markdown/frame.html");
    expect(frame.attributes("sandbox")).toBe("allow-scripts");
    expect(wrapper.text()).toContain("Markdown");
  });

  // §53: the plugin is handed the text the user chose, and only when it is ready.
  it("hands over the text it was opened with", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", text: "hello" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.ready" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.open", text: "hello" }, "*");
  });

  // A plugin never opens the picker itself: it asks, and the app asks the user.
  it("asks the user for a file when the plugin wants one", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(
        command === "core_pick_files"
          ? [{ path: "/data/uploads/a.jpg", name: "a.jpg", mime: "image/jpeg", size: 10 }]
          : command === "core_read_picked"
            ? "QUJD"
            : undefined,
      ),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.pickFile", accept: "image/*" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_pick_files");
    expect(post).toHaveBeenCalledWith({ type: "ft.file", name: "a.jpg", mime: "image/jpeg", data: "QUJD" }, "*");
  });

  it("sends what the plugin made, as a file of the chat", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_send_made", {
      contact: "ft_bob",
      name: "clean.jpg",
      mime: "image/jpeg",
      data: "QUJD",
    });
    expect(wrapper.emitted("done")).toBeTruthy();
  });

  it("offers the text a plugin proposes, without sending it", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.text", text: "# Title" });
    await flushPromises();
    expect(wrapper.emitted("text")).toEqual([["# Title"]]);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_send", expect.anything());
  });
});
