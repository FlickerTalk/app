import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
      { type: "ft.open", text: "hello", dark: false, theme: {}, lang: "en", file: null, ref: null, reminder: null, live: false },
      "*",
    );
  });

  // 2026-10-02 (Ioan): the plugin is handed the app's colours and whether it is dark, before it
  // loads (`ft.hello`), when it opens, and again whenever the app's look changes while it is open.
  describe("the app's colours", () => {
    const html = document.documentElement;
    afterEach(() => {
      html.classList.remove("ft-dark");
      delete html.dataset.direction;
      document.body.removeAttribute("style");
    });

    it("says the app is dark when it is, and hands over its colours", async () => {
      html.classList.add("ft-dark");
      document.body.style.setProperty("--ion-text-color", "#f5f5f5");
      document.body.style.setProperty("--ion-background-color", "#000000");
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
      await flushPromises();
      const { post, says } = framed(wrapper);
      says({ type: "ft.hello" });
      await flushPromises();
      expect(post).toHaveBeenCalledWith(
        { type: "ft.theme", dark: true, theme: { "--ion-text-color": "#f5f5f5", "--ion-background-color": "#000000" } },
        "*",
      );
      says({ type: "ft.ready" });
      await flushPromises();
      expect(post).toHaveBeenCalledWith(
        expect.objectContaining({ type: "ft.open", dark: true, theme: { "--ion-text-color": "#f5f5f5", "--ion-background-color": "#000000" } }),
        "*",
      );
    });

    it("follows the app when its look changes while the plugin is open", async () => {
      html.classList.add("ft-dark");
      document.body.style.setProperty("--ion-text-color", "#f5f5f5");
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "" }, shallow: true });
      await flushPromises();
      const { post } = framed(wrapper);

      // The user picks light: the root's class changes, then the colours the body computes.
      document.body.style.setProperty("--ion-text-color", "#0a0a0a");
      html.classList.remove("ft-dark");
      await flushPromises();
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, theme: { "--ion-text-color": "#0a0a0a" } }, "*");

      // Another colour direction, same mode.
      document.body.style.setProperty("--ion-text-color", "#121821");
      html.dataset.direction = "aurora";
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, theme: { "--ion-text-color": "#121821" } }, "*");

      // A class that changes nothing of the look says nothing.
      const before = post.mock.calls.length;
      html.classList.add("plt-android");
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post.mock.calls.length).toBe(before);
      html.classList.remove("plt-android");

      // Gone, it no longer listens.
      wrapper.unmount();
      html.classList.add("ft-dark");
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post.mock.calls.length).toBe(before);
    });
  });

  // 2026-10-02: opened in a chat, any plugin (tool or game) learns the core's opaque id of that
  // chat, so what it keeps per conversation stays there; opened on its own, there is no `chat`.
  it("hands over the id of the chat it was opened in, and none without a chat", async () => {
    const chat = "Zq3_".padEnd(43, "x");
    tauri.invoke.mockImplementation(async (command: string) => (command === "core_plugin_chat" ? chat : undefined));
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);
    says({ type: "ft.ready" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_chat", { plugin: plugin.id, contact: "ft_bob" });
    expect(post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open", chat }), "*");

    tauri.invoke.mockClear();
    const alone = mount(PluginSheet, { props: { plugin, contact: "" }, shallow: true });
    await flushPromises();
    const lone = framed(alone);
    lone.says({ type: "ft.ready" });
    await flushPromises();
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_plugin_chat", expect.anything());
    expect(lone.post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open" }), "*");
    expect(lone.post.mock.calls[0][0]).not.toHaveProperty("chat");

    // A chat the core gives no id for (blocked, or its session closed): it opens without one.
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === "core_plugin_chat") throw "that is not a contact of yours";
    });
    const refused = mount(PluginSheet, { props: { plugin, contact: "ft_carol" }, shallow: true });
    await flushPromises();
    const nope = framed(refused);
    nope.says({ type: "ft.ready" });
    await flushPromises();
    expect(nope.post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open" }), "*");
    expect(nope.post.mock.calls[0][0]).not.toHaveProperty("chat");
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

  // 2026-10-01 (§108): opened inside a hidden session, everything the plugin keeps, sets or looks
  // up is the session's: the core is told where, the plugin is not.
  it("tells the core the session the plugin is open in, for all it keeps", async () => {
    tauri.invoke.mockImplementation((command: string) => {
      if (command === "core_plugin_record_usage") return Promise.resolve([0, 4096]);
      if (command === "core_plugin_open_chat") return Promise.resolve(null);
      return Promise.resolve(command === "core_remind_list" ? [] : null);
    });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", session: "s1" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    const asked = [
      { type: "ft.recordSet", id: "q1", key: "board/1", value: "x" },
      { type: "ft.recordGet", id: "q2", key: "board/1" },
      { type: "ft.recordKeys", id: "q3", prefix: "" },
      { type: "ft.recordUsage", id: "q4" },
      { type: "ft.recordForget", id: "q5", key: "board/1" },
      { type: "ft.write", id: "q6", key: "pen", value: "black" },
      { type: "ft.read", id: "q7", key: "pen" },
      { type: "ft.forget", id: "q8", key: "pen" },
      { type: "ft.remindSet", id: "q9", reminder: "r1", at: 5, text: "milk" },
      { type: "ft.remindCancel", id: "q10", reminder: "r1" },
      { type: "ft.remindList", id: "q11" },
      { type: "ft.openChat", id: "q12", ref: "ref_1" },
    ];
    for (const question of asked) says(question);
    await flushPromises();

    const commands = [
      "core_plugin_record_set",
      "core_plugin_record_get",
      "core_plugin_record_keys",
      "core_plugin_record_usage",
      "core_plugin_record_forget",
      "core_plugin_write",
      "core_plugin_read",
      "core_plugin_forget",
      "core_remind_set",
      "core_remind_cancel",
      "core_remind_list",
      "core_plugin_open_chat",
    ];
    for (const command of commands) {
      expect(tauri.invoke).toHaveBeenCalledWith(command, expect.objectContaining({ plugin: plugin.id, session: "s1" }));
    }
    // What the frame hears says nothing of a session.
    for (const [message] of post.mock.calls) expect(JSON.stringify(message)).not.toContain("s1");
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

  // 2026-10-02: the location plugin. The core checks the grant and asks the phone; the plugin
  // gets the fix, or null for anything else (refused, off, no fix, not granted).
  it("asks the core where the phone is, and answers null when there is no place", async () => {
    const fix = { lat: 40.41678, lon: -3.70379, accuracy: 35, at: 1_790_000_000_000 };
    tauri.invoke.mockResolvedValue(fix);
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.location", id: "l1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_location", { plugin: plugin.id });
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: fix }, "*");

    tauri.invoke.mockResolvedValue(null);
    says({ type: "ft.location", id: "l2" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l2", answer: null }, "*");

    tauri.invoke.mockImplementation((command: string) =>
      command === "core_plugin_location" ? Promise.reject(new Error("may not ask where the phone is")) : Promise.resolve(undefined),
    );
    says({ type: "ft.location", id: "l3" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l3", answer: null }, "*");
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
