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
    expect(post).toHaveBeenCalledWith({ type: "ft.open", text: "hello", dark: false }, "*");
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

    says({ type: "ft.pickFile", id: "q1", accept: "image/*" });
    await flushPromises();
    // What the plugin asked for reaches the phone: pictures open the photo picker, a sheet over
    // the app, instead of taking the user out of it (§62).
    expect(tauri.invoke).toHaveBeenCalledWith("core_pick_files", { accept: "image/*" });
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q1", name: "a.jpg", mime: "image/jpeg", data: "QUJD" }, "*");
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

  // Issue app#4: the rest of what the core exposes. Each question is answered with its own id,
  // and every one of them goes through a command of the core, never through the frame.
  it("saves, prints and remembers for the plugin, through the core", async () => {
    tauri.invoke.mockResolvedValue(undefined);
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.save", id: "s1", name: "a.pdf", mime: "application/pdf", data: "QUJD" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_save", { name: "a.pdf", mime: "application/pdf", data: "QUJD" });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "s1", answer: true }, "*");

    says({ type: "ft.print", id: "p1", name: "a.pdf", mime: "application/pdf", data: "QUJD" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_print", {
      plugin: plugin.id,
      name: "a.pdf",
      mime: "application/pdf",
      data: "QUJD",
    });

    says({ type: "ft.write", id: "w1", key: "pen", value: "black" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_write", { plugin: plugin.id, key: "pen", value: "black" });

    tauri.invoke.mockResolvedValue("black");
    says({ type: "ft.read", id: "r1", key: "pen" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "r1", answer: "black" }, "*");
  });

  it("makes the call the plugin asked for through the core, never itself", async () => {
    tauri.invoke.mockResolvedValue({ status: 200, body: "QUJD" });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({
      type: "ft.fetch",
      id: "f1",
      url: "https://api.openai.com/v1/chat",
      method: "POST",
      headers: [["content-type", "application/json"]],
      body: "e30=",
    });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_fetch", {
      plugin: plugin.id,
      url: "https://api.openai.com/v1/chat",
      method: "POST",
      headers: [["content-type", "application/json"]],
      body: "e30=",
    });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "f1", answer: { status: 200, body: "QUJD" } }, "*");
  });

  it("says when what the plugin asked for could not be done", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      command === "core_plugin_save" ? Promise.reject(new Error("nope")) : Promise.resolve(undefined),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.save", id: "s2", name: "a.pdf", mime: "application/pdf", data: "QUJD" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "s2", answer: false }, "*");
  });

  it("closes when the plugin asks to be closed", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);
    says({ type: "ft.close" });
    await flushPromises();
    expect(wrapper.emitted("done")).toBeTruthy();
  });
});
