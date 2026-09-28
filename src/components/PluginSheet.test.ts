import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { reactive } from "vue";

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
    expect(frame.attributes("title")).toBe("Markdown");
  });

  // §53: the plugin is handed the text the user chose, and only when it is ready.
  it("hands over the text it was opened with", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", text: "hello" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.ready" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith(
      { type: "ft.open", text: "hello", dark: false, lang: "en", file: null, ref: null, reminder: null, live: false },
      "*",
    );
  });

  // 2026-09-27: opened with a file and a way back to its message, and with the live channel when
  // it was granted and there is another side.
  it("hands over the file, the ref and the channel it was opened with", async () => {
    const file = { name: "class.ftboard", mime: "application/x-ftboard", data: "QUJD" };
    const wrapper = mount(PluginSheet, {
      props: { plugin, contact: "ft_bob", file, reference: "ref_1", live: true },
      shallow: true,
    });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.ready" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open", file, ref: "ref_1", live: true }), "*");

    // On its own (no contact) there is no other side, whatever was granted.
    const alone = mount(PluginSheet, { props: { plugin, contact: "", live: true, reminder: "r1" }, shallow: true });
    await flushPromises();
    const lone = framed(alone);
    lone.says({ type: "ft.ready" });
    await flushPromises();
    expect(lone.post).toHaveBeenCalledWith(expect.objectContaining({ live: false, reminder: "r1" }), "*");
  });

  // Found on a real phone (2026-09-27): a reminder tapped while its plugin is already on screen
  // only changes the reminder; the plugin has to hear it to open that note.
  it("opens the plugin again on a new reminder", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.ready" });
    await flushPromises();
    post.mockClear();
    await wrapper.setProps({ reminder: "r2" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open", reminder: "r2" }), "*");
  });

  // Found on a real phone (2026-09-27): ChatThread keeps what it opens in a deep ref, so the file
  // arrives as a reactive proxy, and a real postMessage cannot clone a proxy (DataCloneError): the
  // plugin never heard `ft.open` and opened empty. What crosses to the frame is plain data.
  it("hands over a file it was given as reactive state", async () => {
    const file = reactive({ name: "class.ftboard", mime: "application/x-ftboard", data: "QUJD" });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", file }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    post.mockImplementation((message: unknown) => structuredClone(message));
    says({ type: "ft.ready" });
    await flushPromises();
    expect(post).toHaveReturnedWith(
      expect.objectContaining({ type: "ft.open", file: { name: "class.ftboard", mime: "application/x-ftboard", data: "QUJD" } }),
    );
  });

  // 2026-09-27: records are the plugin's bytes, kept by the core as base64 and handed back as
  // the string the plugin wrote; reminders and the way back go through the core too.
  it("keeps records, sets reminders and finds the way back through the core", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_plugin_record_get") return Promise.resolve(btoa(unescape(encodeURIComponent("milk ñ"))));
      if (command === "core_plugin_record_keys") return Promise.resolve(["note/1"]);
      if (command === "core_plugin_record_usage") return Promise.resolve([12, 4096]);
      if (command === "core_remind_list") return Promise.resolve([{ plugin: plugin.id, id: "r1", at: 5, text: "" }]);
      if (command === "core_remind_cancel") return Promise.resolve(true);
      if (command === "core_plugin_open_chat") return Promise.resolve({ contact: "ft_bob", message: "m1" });
      return Promise.resolve(undefined);
    });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.recordSet", id: "q1", key: "note/1", value: "milk ñ" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_record_set", { plugin: plugin.id, key: "note/1", value: btoa(unescape(encodeURIComponent("milk ñ"))) });
    says({ type: "ft.recordGet", id: "q2", key: "note/1" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q2", answer: "milk ñ" }, "*");
    says({ type: "ft.recordKeys", id: "q3", prefix: "note/" });
    says({ type: "ft.recordUsage", id: "q4" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q3", answer: ["note/1"] }, "*");
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q4", answer: { used: 12, quota: 4096 } }, "*");

    says({ type: "ft.remindSet", id: "q5", reminder: "r1", at: 5, text: "milk" });
    says({ type: "ft.remindList", id: "q6" });
    says({ type: "ft.remindCancel", id: "q7", reminder: "r1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_remind_set", { plugin: plugin.id, id: "r1", at: 5, text: "milk" });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q6", answer: [{ plugin: plugin.id, id: "r1", at: 5, text: "" }] }, "*");
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q7", answer: true }, "*");

    says({ type: "ft.openChat", id: "q8", ref: "ref_1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_open_chat", { plugin: plugin.id, reference: "ref_1" });
    expect(wrapper.emitted("openChat")).toEqual([["ft_bob"]]);
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q8", answer: true }, "*");
  });

  // 2026-09-27: what the plugin says over the channel goes through the core, only with the
  // grant and a contact; what the other side said is handed to the frame.
  it("carries the live channel both ways, only when it may", async () => {
    tauri.invoke.mockResolvedValue(true);
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", live: true }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.liveSend", id: "q1", data: "AQID" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_live_send", { plugin: plugin.id, contact: "ft_bob", data: "AQID" });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: true }, "*");

    const locked = mount(PluginSheet, { props: { plugin, contact: "ft_bob", live: false }, shallow: true });
    await flushPromises();
    const other = framed(locked);
    tauri.invoke.mockClear();
    other.says({ type: "ft.liveSend", id: "q2", data: "AQID" });
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_plugin_live_send", expect.anything());
    expect(other.post).toHaveBeenCalledWith({ type: "ft.done", id: "q2", answer: false }, "*");
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

  // A2: what a plugin made goes as far as the user allowed. With `auto` the core sends it.
  it("hands what the plugin made to the core, which sends it with the auto permission", async () => {
    tauri.invoke.mockResolvedValue({ sent: true });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", sending: "auto" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_made", {
      plugin: plugin.id,
      contact: "ft_bob",
      name: "clean.jpg",
      mime: "image/jpeg",
      data: "QUJD",
    });
    expect(wrapper.emitted("attach")).toBeUndefined();
    expect(wrapper.emitted("done")).toBeTruthy();
  });

  // With `propose` the file lands in the composer: the user sends it, never the plugin.
  it("stages what the plugin made for the user to send, with the propose permission", async () => {
    const staged = { path: "/data/files/outgoing/1-clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 };
    tauri.invoke.mockResolvedValue({ sent: false, staged });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", sending: "propose" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_made", expect.objectContaining({ plugin: plugin.id }));
    expect(wrapper.emitted("attach")).toEqual([[staged]]);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_send_picked", expect.anything());
  });

  // Plan-drive (2026-09-27): the drive is the core's; the plugin asks with an operation and gets
  // names and sizes back, never bytes, tokens or the code, and only when it was granted it.
  it("answers the drive for a plugin granted it, never bytes, and stages what it sends", async () => {
    const listing = { folders: [{ id: "f1", name: "Docs", parent: null, modified: 1 }], files: [], pending: [] };
    const down = { path: "/data/files/drive/x1/tax.pdf", name: "tax.pdf", mime: "application/pdf", size: 9 };
    let granted = false;
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_plugin_may_use_drive") return Promise.resolve(granted);
      if (command === "core_vault_list") return Promise.resolve(listing);
      if (command === "core_vault_status") return Promise.resolve({ state: "ready", provider: "google", drive: null, problem: null });
      if (command === "core_vault_download") return Promise.resolve(down);
      if (command === "core_pick_files") return Promise.resolve([down]);
      if (command === "core_plugin_open_chat") return Promise.resolve({ contact: "ft_bob", message: "m9" });
      if (command === "core_vault_upload_message") return Promise.resolve("x2");
      return Promise.resolve(undefined);
    });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", sending: "propose", reference: "ref_9" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.drive", id: "d1", op: "list", a: "" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: false }, "*");
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_vault_list", expect.anything());

    granted = true;
    says({ type: "ft.drive", id: "d2", op: "list", a: "" });
    says({ type: "ft.drive", id: "d3", op: "status" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_vault_list", { parent: null });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d2", answer: listing }, "*");
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d3", answer: expect.objectContaining({ state: "ready" }) }, "*");

    // Uploading: the app opens the picker, the core seals what was picked; the frame sees a count.
    says({ type: "ft.drive", id: "d4", op: "upload", a: "f1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_vault_upload", { file: down, parent: "f1" });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d4", answer: 1 }, "*");

    // Keeping the file it was opened with goes by its ref: no bytes cross the frame.
    says({ type: "ft.drive", id: "d5", op: "keep", a: "" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_vault_upload_message", { message: "m9", parent: null });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d5", answer: true }, "*");

    // Sending a file of the drive: down from the cloud, then staged for the user (propose).
    says({ type: "ft.drive", id: "d6", op: "send", a: "x1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_vault_download", { id: "x1" });
    expect(wrapper.emitted("attach")).toEqual([[down]]);
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_send_picked", expect.anything());
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d6", answer: true }, "*");

    // The recovery phrase is typed only in Settings (2026-09-28): a plugin can neither set the
    // drive up nor open it, so the phrase never crosses the frame.
    says({ type: "ft.drive", id: "d7", op: "setup" });
    says({ type: "ft.drive", id: "d8", op: "unlock", a: "a long phrase of mine" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d7", answer: false }, "*");
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d8", answer: false }, "*");
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_vault_setup", expect.anything());
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_vault_unlock", expect.anything());
  });

  // With nothing granted, nothing leaves: not a file, not a text, not even a call to the core.
  it("lets nothing of a plugin without the permission reach the chat", async () => {
    tauri.invoke.mockResolvedValue({ sent: true });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
    says({ type: "ft.text", text: "# Title" });
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_plugin_made", expect.anything());
    expect(wrapper.emitted("text")).toBeUndefined();
    expect(wrapper.emitted("attach")).toBeUndefined();
  });

  it("offers the text a plugin proposes, without sending it", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", sending: "propose" }, shallow: true });
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
