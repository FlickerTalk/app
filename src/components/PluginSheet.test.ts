import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { reactive } from "vue";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauri.invoke,
  convertFileSrc: (path: string, protocol: string) => `http://${protocol}.localhost/${path}`,
}));

// Ionic's toasts are overlays of the real app; here, what the sheet asks of them.
const toast = vi.hoisted(() => ({ create: vi.fn(), present: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

import PluginSheet from "./PluginSheet.vue";
import source from "./PluginSheet.vue?raw";
import { setLocale } from "../i18n";
import { CLOSING_WAIT, NOTICE_DURATION, installed } from "../plugins";
import type { PluginView, Sending } from "../core";
import type { PermissionNeed } from "../permissions";
import type { ContactAsk } from "../pending-send";

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
  beforeEach(() => {
    tauri.invoke.mockReset();
    // Every test gets a toast that shows: a plugin refused for want of a permission says so (app#76).
    // A toast is an element (the sheet fits it to the plugin's pane) that presents and dismisses.
    toast.create
      .mockReset()
      .mockImplementation(async () => Object.assign(document.createElement("div"), { present: toast.present, dismiss: () => Promise.resolve(true) }));
  });

  // 2026-10-02 (plan of the catalogue's translations): a screen reader names the frame as the
  // phone's language does.
  it("names its frame in the phone's language", async () => {
    await setLocale("es");
    try {
      const notes = { id: "com.flickertalk.notes", name: "Notes", locales: { es: { name: "Notas" } } };
      const wrapper = mount(PluginSheet, { props: { plugin: notes, contact: "ft_bob" }, shallow: true });
      await flushPromises();
      expect(wrapper.find("iframe").attributes("title")).toBe("Notas");
    } finally {
      await setLocale("en");
    }
  });

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
      { type: "ft.open", text: "hello", dark: false, dir: "ltr", fill: false, theme: {}, lang: "en", file: null, ref: null, reminder: null, live: false },
      "*",
    );
  });

  // 2026-10-02 (Ioan): the plugin is handed the app's colours and whether it is dark, before it
  // loads (`ft.hello`), when it opens, and again whenever the app's look changes while it is open.
  // 2026-10-09 (Ioan, Ionic in the plugins): an overlay of Ionic is placed in the frame, so a frame
  // as tall as its content dropped a toast or an action sheet off the screen. In the tool window the
  // frame is as tall as the window and the plugin scrolls inside; elsewhere (the game room) it
  // still follows its content.
  describe("its height", () => {
    it("follows the content by default", async () => {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "" }, shallow: true });
      await flushPromises();
      framed(wrapper).says({ type: "ft.height", height: 900 });
      await flushPromises();
      expect(wrapper.find("iframe").attributes("style")).toContain("height: 900px");
      expect(wrapper.find("section").classes()).not.toContain("ft-plugin--fill");
    });

    it("fills what it is given when asked to, whatever the content says", async () => {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "", fill: true }, shallow: true });
      await flushPromises();
      const { post, says } = framed(wrapper);
      // The frame hears it before the plugin draws, and again when it is opened.
      says({ type: "ft.hello" });
      await flushPromises();
      expect(post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.theme", fill: true }), "*");
      says({ type: "ft.ready" });
      await flushPromises();
      expect(post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open", fill: true }), "*");
      says({ type: "ft.height", height: 900 });
      await flushPromises();
      expect(wrapper.find("iframe").attributes("style") ?? "").not.toContain("px");
      expect(wrapper.find("section").classes()).toContain("ft-plugin--fill");
    });
  });

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
        { type: "ft.theme", dark: true, dir: "ltr", fill: false, theme: { "--ion-text-color": "#f5f5f5", "--ion-background-color": "#000000" } },
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
      expect(post).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, dir: "ltr", fill: false, theme: { "--ion-text-color": "#0a0a0a" } }, "*");

      // Another colour direction, same mode.
      document.body.style.setProperty("--ion-text-color", "#121821");
      html.dataset.direction = "aurora";
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, dir: "ltr", fill: false, theme: { "--ion-text-color": "#121821" } }, "*");

      // The language turns the text right to left (2026-10-09): the frame hears it.
      html.dir = "rtl";
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(post).toHaveBeenLastCalledWith({ type: "ft.theme", dark: false, dir: "rtl", fill: false, theme: { "--ion-text-color": "#121821" } }, "*");
      html.removeAttribute("dir");
      await new Promise((resolve) => setTimeout(resolve, 0));

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

  // A presentation in a call (API 1.6.0): the app says whether this side leads or follows.
  it("tells a plugin opened in a call whether it leads or follows, and says nothing of it otherwise", async () => {
    const shown = mount(PluginSheet, { props: { plugin, contact: "ft_bob", live: true, presenting: "follow" }, shallow: true });
    await flushPromises();
    const call = framed(shown);
    call.says({ type: "ft.ready" });
    await flushPromises();
    expect(call.post).toHaveBeenCalledWith(expect.objectContaining({ type: "ft.open", live: true, presenting: "follow" }), "*");

    const plain = mount(PluginSheet, { props: { plugin, contact: "ft_bob", live: true }, shallow: true });
    await flushPromises();
    const chat = framed(plain);
    chat.says({ type: "ft.ready" });
    await flushPromises();
    const opened = chat.post.mock.calls.map(([message]) => message).find((message) => message.type === "ft.open");
    expect(opened).not.toHaveProperty("presenting");
  });

  // Whoever shows the plugin learns when the core will not open it (the call screen says so).
  it("says when the core refuses to open the plugin", async () => {
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === "core_plugin_open") throw "refused";
    });
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", live: true, presenting: "follow" }, shallow: true });
    await flushPromises();
    expect(wrapper.emitted("refused")).toHaveLength(1);
    tauri.invoke.mockReset();
    const fine = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    expect(fine.emitted("refused")).toBeUndefined();
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
  // 2026-10-02: one command picks and hands over; the core deletes the picker's copies, so no path
  // travels through the WebView and none is left behind.
  it("asks the user for a file when the plugin wants one", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_pick_for_plugin" ? { name: "a.jpg", mime: "image/jpeg", data: "QUJD" } : undefined),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.pickFile", id: "q1", accept: "image/*" });
    await flushPromises();
    // What the plugin asked for reaches the phone: pictures open the photo picker, a sheet over
    // the app, instead of taking the user out of it (§62).
    expect(tauri.invoke).toHaveBeenCalledWith("core_pick_for_plugin", { accept: "image/*" });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_pick_files", expect.anything());
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q1", name: "a.jpg", mime: "image/jpeg", data: "QUJD" }, "*");
  });

  it("hands the plugin an empty file when the user picks nothing or the pick fails", async () => {
    let fails = false;
    tauri.invoke.mockImplementation((command: string) =>
      command === "core_pick_for_plugin" ? (fails ? Promise.reject(new Error("too big")) : Promise.resolve(null)) : Promise.resolve(undefined),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.pickFile", id: "q1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_pick_for_plugin", { accept: "" });
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q1", name: "", mime: "", data: "" }, "*");

    fails = true;
    says({ type: "ft.pickFile", id: "q2" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q2", name: "", mime: "", data: "" }, "*");
  });

  // 2026-10-06: a plugin may ask for a photo taken now with the phone's camera app. The app opens
  // it and hands the photo back like a picked file; the core deletes its copy, as with a pick.
  it("takes a photo with the camera when the plugin wants one", async () => {
    tauri.invoke.mockImplementation((command: string) =>
      Promise.resolve(command === "core_take_photo_for_plugin" ? { name: "photo.jpg", mime: "image/jpeg", data: "QUJD" } : undefined),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.takePhoto", id: "q1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_take_photo_for_plugin");
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_take_photo");
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q1", name: "photo.jpg", mime: "image/jpeg", data: "QUJD" }, "*");
  });

  // Backing out, no camera (a simulator) or a camera not allowed: the plugin gets nothing, never
  // an error.
  it("hands the plugin no photo when the user backs out or there is no camera", async () => {
    let fails = false;
    tauri.invoke.mockImplementation((command: string) =>
      command === "core_take_photo_for_plugin"
        ? fails
          ? Promise.reject(new Error("unsupported"))
          : Promise.resolve(null)
        : Promise.resolve(undefined),
    );
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { post, says } = framed(wrapper);

    says({ type: "ft.takePhoto", id: "q1" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q1", name: "", mime: "", data: "" }, "*");

    fails = true;
    says({ type: "ft.takePhoto", id: "q2" });
    await flushPromises();
    expect(post).toHaveBeenCalledWith({ type: "ft.file", id: "q2", name: "", mime: "", data: "" }, "*");
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
      if (command === "core_vault_upload_picked") return Promise.resolve(2);
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

    // Uploading: the core opens the picker, seals what was picked and deletes the picker's copies
    // (2026-10-02); no path crosses the WebView, and the frame sees a count.
    says({ type: "ft.drive", id: "d4", op: "upload", a: "f1" });
    await flushPromises();
    expect(tauri.invoke).toHaveBeenCalledWith("core_vault_upload_picked", { parent: "f1" });
    expect(tauri.invoke).not.toHaveBeenCalledWith("core_pick_files", expect.anything());
    expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d4", answer: 2 }, "*");

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

  // app#76: refused for want of the permission, a plugin's main action used to do nothing at all.
  // Now the user hears why, and where to allow it.
  it("tells the user when a plugin may not write in the chat, and where to allow it", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
    await flushPromises();
    // Since 2026-10-08 the switches are on the app's sheet in the Apps tab: the notice says where.
    const message = "This tool may not write in the chat. Turn on “Write in the chat” for it in Apps: touch and hold its icon.";
    expect(toast.create).toHaveBeenCalledWith(expect.objectContaining({ message }));
    expect(toast.present).toHaveBeenCalled();

    says({ type: "ft.text", text: "# Title" });
    await flushPromises();
    expect(toast.create).toHaveBeenCalledTimes(2);
  });

  describe("notices", () => {
    // Each toast the sheet creates, in order, so a test can see which one went.
    type Shown = {
      options: Record<string, unknown>;
      present: ReturnType<typeof vi.fn>;
      dismiss: ReturnType<typeof vi.fn>;
      el: HTMLElement;
    };
    let shown: Shown[];
    beforeEach(() => {
      shown = [];
      toast.create.mockReset().mockImplementation(async (options: Record<string, unknown>) => {
        const present = vi.fn().mockResolvedValue(undefined);
        const dismiss = vi.fn().mockResolvedValue(true);
        const el = Object.assign(document.createElement("div"), { present, dismiss });
        shown.push({ options, present, dismiss, el });
        return el;
      });
    });

    async function sheet(props: Record<string, unknown> = {}) {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", ...props }, shallow: true });
      await flushPromises();
      const frame = framed(wrapper);
      frame.says({ type: "ft.ready" });
      await flushPromises();
      return { wrapper, ...frame };
    }

    // 2026-10-06 (Ioan): the app owns the toast; a plugin or a game only hands it the text. One
    // toast, at the top, floating over the content; no permission, it never leaves the phone.
    it("shows a plugin's notice as one toast at the top", async () => {
      const { says } = await sheet({ sending: "nothing" });
      says({ type: "ft.notify", text: "Your turn" });
      await flushPromises();
      expect(shown).toHaveLength(1);
      expect(shown[0].options).toEqual(expect.objectContaining({ message: "Your turn", position: "top", duration: NOTICE_DURATION }));
      expect(shown[0].present).toHaveBeenCalled();
      expect(tauri.invoke).not.toHaveBeenCalledWith(expect.stringContaining("notify"), expect.anything());
    });

    it("dismisses the previous notice before it shows the next one", async () => {
      const { says } = await sheet();
      says({ type: "ft.notify", text: "Your turn" });
      says({ type: "ft.notify", text: "That move is not allowed" });
      await flushPromises();
      expect(shown.map((one) => one.options.message)).toEqual(["Your turn", "That move is not allowed"]);
      expect(shown[0].dismiss).toHaveBeenCalled();
      expect(shown[0].dismiss.mock.invocationCallOrder[0]).toBeLessThan(shown[1].present.mock.invocationCallOrder[0]);
      expect(shown[1].dismiss).not.toHaveBeenCalled();
    });

    it("keeps a sticky notice until the plugin clears it with an empty text", async () => {
      const { says } = await sheet();
      says({ type: "ft.notify", text: "Waiting for the other player…", sticky: true });
      await flushPromises();
      expect(shown[0].options.duration).toBeUndefined();

      says({ type: "ft.notify", text: "" });
      await flushPromises();
      expect(shown[0].dismiss).toHaveBeenCalled();
      expect(shown, "an empty text shows nothing").toHaveLength(1);
    });

    // Under the window's bar, not over its way out: a sticky notice must never cover the ✕.
    it("floats the notice from the top of the plugin, under the window's bar", async () => {
      const { wrapper, says } = await sheet();
      const anchor = wrapper.find(".ft-plugin__notices").element as HTMLElement;
      Object.defineProperty(anchor, "offsetParent", { value: wrapper.element, configurable: true });
      says({ type: "ft.notify", text: "Your turn" });
      await flushPromises();
      expect(shown[0].options.positionAnchor).toBe(anchor);
    });

    // Lenovo tablet (2026-10-06): beside the chat list, the notice was centred on the whole window
    // and spilled over the list. It is centred over the plugin's pane, with Ionic's own gutter.
    describe("width", () => {
      const width = window.innerWidth;
      afterEach(() => Object.defineProperty(window, "innerWidth", { value: width, configurable: true }));

      async function noticeIn(viewport: number, left: number, right: number) {
        Object.defineProperty(window, "innerWidth", { value: viewport, configurable: true });
        const { wrapper, says } = await sheet();
        const pane = wrapper.find(".ft-plugin").element as HTMLElement;
        pane.getBoundingClientRect = () => ({ left, right, width: right - left, top: 56, bottom: 800, height: 744, x: left, y: 56, toJSON: () => ({}) });
        says({ type: "ft.notify", text: "Your turn" });
        await flushPromises();
        return shown[0].el.style;
      }

      it("centres the notice over the plugin's pane beside the chat list", async () => {
        const style = await noticeIn(1280, 440, 1280);
        expect(style.getPropertyValue("--start")).toBe("448px");
        expect(style.getPropertyValue("--end")).toBe("8px");
      });

      it("leaves the notice as wide as the screen on a phone", async () => {
        const style = await noticeIn(390, 0, 390);
        expect(style.getPropertyValue("--start")).toBe("8px");
        expect(style.getPropertyValue("--end")).toBe("8px");
      });
    });

    // Out of sight, the plugin no longer reaches the user, and its notice goes with it.
    it("ignores notices while the plugin closes, and takes its notice away", async () => {
      const { wrapper, says } = await sheet();
      says({ type: "ft.notify", text: "Waiting…", sticky: true });
      await flushPromises();
      void (wrapper.vm as unknown as { close: () => Promise<void> }).close();
      await flushPromises();
      expect(shown[0].dismiss).toHaveBeenCalled();

      says({ type: "ft.notify", text: "Bye" });
      await flushPromises();
      expect(shown).toHaveLength(1);
    });

    // The refusal of app#76 is a notice too: one toast at a time, at the top.
    it("tells a refusal in the same single toast at the top", async () => {
      const { says } = await sheet();
      says({ type: "ft.notify", text: "Your turn", sticky: true });
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      expect(shown).toHaveLength(2);
      expect(shown[0].dismiss).toHaveBeenCalled();
      expect(shown[1].options).toEqual(expect.objectContaining({ position: "top" }));
    });
  });

  // Opened from Settings there is no chat to write in: nothing to tell.
  it("says nothing of the chat for a plugin opened outside a conversation", async () => {
    const wrapper = mount(PluginSheet, { props: { plugin, contact: "" }, shallow: true });
    await flushPromises();
    const { says } = framed(wrapper);

    says({ type: "ft.text", text: "# Title" });
    await flushPromises();
    expect(toast.create).not.toHaveBeenCalled();
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

  // Ioan, 2026-10-09: a tool opened on its own (from Apps, from a reminder) has no chat behind it.
  // What it proposes asks the user who it is for; the sheet hands it over with the contact picked
  // (`sendTo`) and whoever shows it opens that conversation with it in the composer (§53). No pick,
  // nothing: the window stays and the plugin hears what it hears today.
  describe("outside a conversation", () => {
    const staged = { path: "/data/files/outgoing/1-clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 };
    const down = { path: "/data/files/drive/x1/tax.pdf", name: "tax.pdf", mime: "application/pdf", size: 9 };

    async function alone(sending: Sending, answers: Record<string, unknown> = {}) {
      tauri.invoke.mockImplementation((command: string) => Promise.resolve(answers[command]));
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "", sending }, shallow: true });
      await flushPromises();
      const frame = framed(wrapper);
      frame.says({ type: "ft.ready" });
      await flushPromises();
      return { wrapper, ...frame };
    }
    const picks = (wrapper: ReturnType<typeof mount>) => (wrapper.emitted("pick") ?? []).map(([ask]) => ask as ContactAsk);
    const invoked = (command: string) => tauri.invoke.mock.calls.filter(([one]) => one === command);

    it("asks who a proposed text is for, and hands it over without sending it", async () => {
      const { wrapper, says } = await alone("propose");
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      expect(picks(wrapper)).toHaveLength(1);
      expect(wrapper.emitted("sendTo")).toBeUndefined();
      picks(wrapper)[0].answer("ft_bob");
      await flushPromises();
      expect(wrapper.emitted("sendTo")).toEqual([["ft_bob", { kind: "text", text: "# Title" }]]);
      expect(wrapper.emitted("text")).toBeUndefined();
      expect(wrapper.emitted("done")).toBeUndefined();
      expect(invoked("core_send")).toHaveLength(0);
    });

    it("stages a file it made once the contact is picked, and hands it over", async () => {
      const { wrapper, says } = await alone("propose", { core_plugin_made: { sent: false, staged } });
      says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      await flushPromises();
      // Nothing is written before the user says who it is for.
      expect(invoked("core_plugin_made")).toHaveLength(0);
      picks(wrapper)[0].answer("ft_bob");
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_made", { plugin: plugin.id, contact: "ft_bob", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      expect(wrapper.emitted("sendTo")).toEqual([["ft_bob", { kind: "file", file: staged }]]);
      expect(wrapper.emitted("attach")).toBeUndefined();
      expect(wrapper.emitted("done")).toBeUndefined();
      expect(invoked("core_send_picked")).toHaveLength(0);
    });

    it("keeps the window and sends nothing when no contact is picked", async () => {
      const { wrapper, says } = await alone("propose", { core_plugin_made: { sent: false, staged } });
      says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      await flushPromises();
      picks(wrapper)[0].answer(null);
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      picks(wrapper)[1].answer(null);
      await flushPromises();
      expect(invoked("core_plugin_made")).toHaveLength(0);
      expect(wrapper.emitted("sendTo")).toBeUndefined();
      expect(wrapper.emitted("done")).toBeUndefined();
    });

    // A plugin that writes by itself (`auto`) does so in the conversation picked, as it would there.
    it("lets a plugin with the auto permission send to the contact picked, as in that chat", async () => {
      const { wrapper, says } = await alone("auto", { core_plugin_made: { sent: true } });
      says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      await flushPromises();
      picks(wrapper)[0].answer("ft_bob");
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_made", expect.objectContaining({ contact: "ft_bob" }));
      expect(wrapper.emitted("sendTo")).toEqual([["ft_bob", null]]);
    });

    it("hands over a file of the drive for the contact picked, and tells the plugin", async () => {
      const answers = { core_plugin_may_use_drive: true, core_vault_download: down };
      const { wrapper, post, says } = await alone("propose", answers);
      says({ type: "ft.drive", id: "d1", op: "send", a: "x1" });
      await flushPromises();
      expect(invoked("core_vault_download")).toHaveLength(0);
      picks(wrapper)[0].answer("ft_bob");
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_vault_download", { id: "x1" });
      expect(wrapper.emitted("sendTo")).toEqual([["ft_bob", { kind: "file", file: down }]]);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: true }, "*");
      expect(invoked("core_send_picked")).toHaveLength(0);
    });

    it("answers the drive's send with the refusal when no contact is picked", async () => {
      const { wrapper, post, says } = await alone("propose", { core_plugin_may_use_drive: true, core_vault_download: down });
      says({ type: "ft.drive", id: "d1", op: "send", a: "x1" });
      await flushPromises();
      picks(wrapper)[0].answer(null);
      await flushPromises();
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: false }, "*");
      expect(invoked("core_vault_download")).toHaveLength(0);
      expect(wrapper.emitted("sendTo")).toBeUndefined();
    });

    it("takes a pick left unanswered when the plugin closes as none", async () => {
      const { wrapper, post, says } = await alone("propose", { core_plugin_may_use_drive: true, core_vault_download: down });
      says({ type: "ft.drive", id: "d1", op: "send", a: "x1" });
      await flushPromises();
      const [ask] = picks(wrapper);
      void (wrapper.vm as unknown as { close: () => Promise<void> }).close();
      await flushPromises();
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: false }, "*");
      ask.answer("ft_bob");
      await flushPromises();
      expect(wrapper.emitted("sendTo")).toBeUndefined();
    });

    it("asks who for nothing of a plugin with no permission to write in the chat", async () => {
      const { wrapper, says } = await alone("nothing");
      says({ type: "ft.text", text: "# Title" });
      says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      await flushPromises();
      expect(wrapper.emitted("pick")).toBeUndefined();
    });
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

  // 2026-10-02 (seen on two phones and the iOS simulator): closed by the app, a plugin in a live
  // session vanished without a word and the other side kept saying both were there. Now the app
  // tells it first, keeps it running out of sight until it answers or for CLOSING_WAIT at most,
  // and only then lets it go (`closed`). Nothing in the app waits for it.
  describe("closing", () => {
    type Closable = { close: () => Promise<void> };
    const closing = { type: "ft.closing" };
    const closingsIn = (post: ReturnType<typeof vi.fn>) => post.mock.calls.filter(([message]) => message.type === "ft.closing").length;

    /** A sheet whose plugin is up (`ft.ready`), as it is whenever the user can close it. */
    async function opened(props: Record<string, unknown> = {}) {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", ...props }, shallow: true });
      await flushPromises();
      const frame = framed(wrapper);
      frame.says({ type: "ft.ready" });
      await flushPromises();
      return { wrapper, ...frame, close: () => (wrapper.vm as unknown as Closable).close() };
    }

    // 2026-10-03 (updates): the core never updates a plugin under an open frame. The frame is open
    // from the moment the sheet shows it until the sheet goes, saying goodbye included.
    it("tells the core the plugin is open until its frame is gone, goodbye included", async () => {
      const opens = () => tauri.invoke.mock.calls.filter(([command]) => command === "core_plugin_open").map(([, args]) => args);
      const { wrapper, says, close } = await opened();
      expect(opens()).toEqual([{ plugin: plugin.id, open: true }]);
      void close();
      await flushPromises();
      expect(opens(), "still saying goodbye").toHaveLength(1);
      says({ type: "ft.closed" });
      await flushPromises();
      expect(opens(), "the sheet is still there until the page takes it away").toHaveLength(1);
      wrapper.unmount();
      expect(opens()).toEqual([
        { plugin: plugin.id, open: true },
        { plugin: plugin.id, open: false },
      ]);
    });

    it("tells the plugin, and lets it go only once it answers", async () => {
      const { wrapper, post, says, close } = await opened();
      let over = false;
      void close().then(() => (over = true));
      await flushPromises();
      expect(post).toHaveBeenCalledWith(closing, "*");
      expect(wrapper.emitted("closed")).toBeUndefined();
      expect(over).toBe(false);

      says({ type: "ft.closed" });
      await flushPromises();
      expect(wrapper.emitted("closed")).toHaveLength(1);
      expect(over).toBe(true);
      // Closing is not the plugin asking to close.
      expect(wrapper.emitted("done")).toBeUndefined();
    });

    it("lets it go after CLOSING_WAIT when it does not answer", async () => {
      vi.useFakeTimers();
      try {
        const { wrapper, says, close } = await opened();
        void close();
        await vi.advanceTimersByTimeAsync(CLOSING_WAIT - 1);
        expect(wrapper.emitted("closed")).toBeUndefined();
        await vi.advanceTimersByTimeAsync(1);
        expect(wrapper.emitted("closed")).toHaveLength(1);
        // An answer that comes late changes nothing.
        says({ type: "ft.closed" });
        await vi.advanceTimersByTimeAsync(CLOSING_WAIT);
        expect(wrapper.emitted("closed")).toHaveLength(1);
      } finally {
        vi.useRealTimers();
      }
    });

    it("waits no more than that, whatever the plugin does", () => {
      expect(CLOSING_WAIT).toBe(400);
    });

    it("carries what the plugin says to its twin while it waits, with the same grant as before", async () => {
      tauri.invoke.mockResolvedValue(true);
      const { wrapper, post, says, close } = await opened({ live: true });
      void close();
      says({ type: "ft.liveSend", id: "q1", data: "Ynll" });
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_live_send", { plugin: plugin.id, contact: "ft_bob", data: "Ynll" });
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: true }, "*");
      says({ type: "ft.closed" });
      await flushPromises();
      expect(wrapper.emitted("closed")).toHaveLength(1);

      // Without the live grant, closing gives it nothing it did not have.
      tauri.invoke.mockClear();
      const locked = await opened({ live: false });
      void locked.close();
      locked.says({ type: "ft.liveSend", id: "q2", data: "Ynll" });
      await flushPromises();
      expect(tauri.invoke).not.toHaveBeenCalledWith("core_plugin_live_send", expect.anything());
      expect(locked.post).toHaveBeenCalledWith({ type: "ft.done", id: "q2", answer: false }, "*");
    });

    // Out of sight, it can no longer put anything in front of the user or into the chat.
    it("lets nothing reach the user or the chat while it says goodbye", async () => {
      tauri.invoke.mockResolvedValue({ sent: true });
      const { wrapper, says, close } = await opened({ sending: "auto" });
      tauri.invoke.mockClear();
      void close();
      says({ type: "ft.made", name: "a.png", mime: "image/png", data: "AAAA" });
      says({ type: "ft.text", text: "hello" });
      says({ type: "ft.pickFile", id: "q1", accept: "" });
      says({ type: "ft.openChat", id: "q2", ref: "ref_1" });
      says({ type: "ft.drive", id: "q3", op: "upload", a: "", b: "" });
      says({ type: "ft.print", id: "q4", name: "a.pdf", mime: "application/pdf", data: "AAAA" });
      says({ type: "ft.save", id: "q5", name: "a.pdf", mime: "application/pdf", data: "AAAA" });
      says({ type: "ft.location", id: "q6" });
      says({ type: "ft.takePhoto", id: "q7" });
      says({ type: "ft.notify", text: "Bye" });
      await flushPromises();
      expect(tauri.invoke).not.toHaveBeenCalled();
      expect(toast.create).not.toHaveBeenCalled();
      expect(wrapper.emitted("attach")).toBeUndefined();
      expect(wrapper.emitted("text")).toBeUndefined();
      expect(wrapper.emitted("openChat")).toBeUndefined();
    });

    it("says goodbye once when it is closed twice", async () => {
      const { wrapper, post, says, close } = await opened();
      const first = close();
      const second = close();
      await flushPromises();
      expect(closingsIn(post)).toBe(1);
      says({ type: "ft.closed" });
      await Promise.all([first, second]);
      expect(wrapper.emitted("closed")).toHaveLength(1);
    });

    // A goodbye that ends with `ft.close()` must not close the window a second time.
    it("does not take the plugin closing itself meanwhile as another close", async () => {
      const { wrapper, says, close } = await opened();
      void close();
      says({ type: "ft.close" });
      await flushPromises();
      expect(wrapper.emitted("done")).toBeUndefined();
      says({ type: "ft.closed" });
      await flushPromises();
      expect(wrapper.emitted("closed")).toHaveLength(1);
    });

    it("lets a plugin that never came up go at once", async () => {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
      await flushPromises();
      const { post } = framed(wrapper);
      await (wrapper.vm as unknown as Closable).close();
      expect(wrapper.emitted("closed")).toHaveLength(1);
      expect(closingsIn(post)).toBe(0);
    });

    // Gone some other way (a page torn down): one word on the way out, without waiting.
    it("tells the plugin it is closing when it is torn down without being closed", async () => {
      const { wrapper, post } = await opened();
      wrapper.unmount();
      expect(closingsIn(post)).toBe(1);
    });

    it("says nothing more when it is torn down while it waits", async () => {
      vi.useFakeTimers();
      try {
        const { wrapper, post, close } = await opened();
        void close();
        wrapper.unmount();
        await vi.advanceTimersByTimeAsync(CLOSING_WAIT);
        expect(closingsIn(post)).toBe(1);
        expect(wrapper.emitted("closed")).toBeUndefined();
      } finally {
        vi.useRealTimers();
      }
    });
  });
  // 2026-10-06 (measured on real phones): the working indicator sat above the frame, in the flow,
  // and every Plugin API request pushed the plugin down ~30 px and back. Ioan's rule: a waiting
  // indicator floats over the content or reserves its space from the first frame; nothing moves the
  // content after the first paint. It also shows only for a request still pending after 400 ms.
  describe("the working indicator", () => {
    const working = (wrapper: ReturnType<typeof mount>) => wrapper.find(".ft-plugin__working");

    /** A core whose next answer waits until the test lets it go. */
    function slowCore() {
      let release: (value: unknown) => void = () => undefined;
      tauri.invoke.mockImplementation((command: string) =>
        command === "core_plugin_record_get" ? new Promise((resolve) => (release = resolve)) : Promise.resolve(true),
      );
      return { release: (value: unknown) => release(value) };
    }

    async function ready() {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob" }, shallow: true });
      await flushPromises();
      const frame = framed(wrapper);
      frame.says({ type: "ft.ready" });
      await flushPromises();
      return { wrapper, ...frame };
    }

    it("floats over the frame and takes no room in the flow", () => {
      const styles = source.slice(source.indexOf("<style"));
      expect(styles).toMatch(/\.ft-plugin\s*{[^}]*position:\s*relative/);
      expect(styles).toMatch(/\.ft-plugin__working\s*{[^}]*position:\s*absolute/);
      expect(styles).toMatch(/\.ft-plugin__working\s*{[^}]*pointer-events:\s*none/);
      expect(styles).not.toMatch(/\.ft-plugin__working\s*{[^}]*margin/);
    });

    it("never shows for a request answered within 400 ms", async () => {
      vi.useFakeTimers();
      try {
        const core = slowCore();
        const { wrapper, post, says } = await ready();
        says({ type: "ft.recordGet", id: "q1", key: "move/1" });
        await vi.advanceTimersByTimeAsync(0);
        expect(working(wrapper).exists()).toBe(false);
        await vi.advanceTimersByTimeAsync(399);
        expect(working(wrapper).exists()).toBe(false);
        core.release(null);
        await vi.advanceTimersByTimeAsync(0);
        expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: null }, "*");
        await vi.advanceTimersByTimeAsync(1000);
        expect(working(wrapper).exists()).toBe(false);
      } finally {
        vi.useRealTimers();
      }
    });

    it("shows for a request still pending after 400 ms, and goes with the answer", async () => {
      vi.useFakeTimers();
      try {
        const core = slowCore();
        const { wrapper, says } = await ready();
        const frameStyle = wrapper.find("iframe").attributes("style");
        says({ type: "ft.recordGet", id: "q1", key: "move/1" });
        await vi.advanceTimersByTimeAsync(400);
        expect(working(wrapper).exists()).toBe(true);
        expect(working(wrapper).attributes("role")).toBe("status");
        // The frame keeps its place and its size whether the indicator shows or not.
        expect(wrapper.find("iframe").attributes("style")).toBe(frameStyle);
        core.release(null);
        await vi.advanceTimersByTimeAsync(0);
        expect(working(wrapper).exists()).toBe(false);
      } finally {
        vi.useRealTimers();
      }
    });
  });

  // Ioan, 2026-10-08: «si no tiene permiso se debería pedir siempre». Installing grants nothing
  // (§53), so a tool that asks for something it was not granted asks the user there and then: the
  // sheet holds the request, says which permission it needs (`needs`), and once the user allows it
  // grants it and asks the core again; the plugin only ever sees the real answer.
  describe("asking for a permission on the spot", () => {
    const NOTHING = { network: [], messages: false, send: "nothing", live: false, remind: false, drive: false, location: false, storage: "small" } as const;
    const TOOL: PluginView = {
      id: plugin.id,
      name: "Markdown",
      version: "1.0.0",
      asks: { network: ["api.example.com"], messages: false, send: "propose", live: true, remind: true, drive: true, location: true, storage: "large" },
      granted: { ...NOTHING, network: [] },
      installedAt: 1,
    };
    const FIX = { lat: 40.41678, lon: -3.70379, accuracy: 35, at: 1_790_000_000_000 };

    /** A core that keeps the plugin's grants, as the real one does, and answers the rest. */
    function core(answers: Record<string, unknown> = {}, view: PluginView = TOOL) {
      let now: PluginView = JSON.parse(JSON.stringify(view));
      installed.value = [JSON.parse(JSON.stringify(now))];
      tauri.invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
        if (command === "core_plugins") return Promise.resolve([JSON.parse(JSON.stringify(now))]);
        if (command === "core_plugin_grant") {
          now = { ...now, granted: JSON.parse(JSON.stringify(args?.granted)) };
          return Promise.resolve(undefined);
        }
        const answer = answers[command];
        return typeof answer === "function" ? Promise.resolve((answer as () => unknown)()) : Promise.resolve(answer);
      });
      return { granted: () => now.granted };
    }

    async function open(props: Record<string, unknown> = {}) {
      const wrapper = mount(PluginSheet, { props: { plugin, contact: "ft_bob", ...props }, shallow: true });
      await flushPromises();
      const frame = framed(wrapper);
      frame.says({ type: "ft.ready" });
      await flushPromises();
      return { wrapper, ...frame };
    }
    const needs = (wrapper: ReturnType<typeof mount>) => (wrapper.emitted("needs") ?? []).map(([need]) => need as PermissionNeed);
    const invoked = (command: string) => tauri.invoke.mock.calls.filter(([one]) => one === command);

    afterEach(() => {
      installed.value = [];
    });

    it("asks for the location it lacks, and after a yes hands the plugin the real position", async () => {
      const kept = core({ core_plugin_location: () => (kept.granted().location ? FIX : Promise.reject(new Error("refused"))) });
      const { wrapper, post, says } = await open();

      says({ type: "ft.location", id: "l1" });
      await flushPromises();
      const [need] = needs(wrapper);
      expect(need).toMatchObject({ key: "location", label: "Your location, only when you ask" });
      expect(need.icon).toBeTruthy();
      // Held: the core is not asked yet, and the plugin has heard nothing.
      expect(invoked("core_plugin_location")).toHaveLength(0);
      expect(post).not.toHaveBeenCalledWith(expect.objectContaining({ type: "ft.done", id: "l1" }), "*");

      need.answer(true);
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_grant", { plugin: plugin.id, granted: expect.objectContaining({ location: true, send: "nothing" }) });
      expect(invoked("core_plugin_location")).toHaveLength(1);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: FIX }, "*");

      // Granted now: the next time nothing is asked.
      says({ type: "ft.location", id: "l2" });
      await flushPromises();
      expect(needs(wrapper)).toHaveLength(1);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l2", answer: FIX }, "*");
    });

    it("hands the plugin the refusal when the user says no, and grants nothing", async () => {
      core({ core_plugin_location: FIX });
      const { wrapper, post, says } = await open();
      says({ type: "ft.location", id: "l1" });
      await flushPromises();
      needs(wrapper)[0].answer(false);
      await flushPromises();
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: null }, "*");
      expect(invoked("core_plugin_grant")).toHaveLength(0);
      expect(invoked("core_plugin_location")).toHaveLength(0);
      // A second answer to the same question changes nothing.
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(invoked("core_plugin_grant")).toHaveLength(0);
    });

    it("asks once for two requests that need the same permission", async () => {
      const kept = core({ core_plugin_location: () => (kept.granted().location ? FIX : null) });
      const { wrapper, post, says } = await open();
      says({ type: "ft.location", id: "l1" });
      says({ type: "ft.location", id: "l2" });
      await flushPromises();
      expect(needs(wrapper)).toHaveLength(1);
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(invoked("core_plugin_grant")).toHaveLength(1);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: FIX }, "*");
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l2", answer: FIX }, "*");
    });

    it("asks nothing for a permission the plugin never asked for, nor for one it has", async () => {
      core({ core_plugin_location: null }, { ...TOOL, asks: { ...TOOL.asks, location: false } });
      const first = await open();
      first.says({ type: "ft.location", id: "l1" });
      await flushPromises();
      expect(needs(first.wrapper)).toHaveLength(0);
      expect(first.post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: null }, "*");

      core({ core_plugin_location: FIX }, { ...TOOL, granted: { ...TOOL.granted, location: true } });
      const second = await open();
      second.says({ type: "ft.location", id: "l2" });
      await flushPromises();
      expect(needs(second.wrapper)).toHaveLength(0);
      expect(second.post).toHaveBeenCalledWith({ type: "ft.done", id: "l2", answer: FIX }, "*");
    });

    it("asks to write in the chat, then stages what the plugin made", async () => {
      const staged = { path: "/data/files/outgoing/1-clean.jpg", name: "clean.jpg", mime: "image/jpeg", size: 3 };
      core({ core_plugin_made: { sent: false, staged } });
      const { wrapper, says } = await open({ sending: "nothing" });
      says({ type: "ft.made", name: "clean.jpg", mime: "image/jpeg", data: "QUJD" });
      await flushPromises();
      expect(needs(wrapper).map((one) => [one.key, one.label])).toEqual([["send", "Write in the chat"]]);
      expect(invoked("core_plugin_made")).toHaveLength(0);
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_grant", { plugin: plugin.id, granted: expect.objectContaining({ send: "propose" }) });
      expect(invoked("core_plugin_made")).toHaveLength(1);
      expect(wrapper.emitted("attach")).toEqual([[staged]]);
      // Allowed now: a proposed text lands in the composer without asking again.
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      expect(needs(wrapper)).toHaveLength(1);
      expect(wrapper.emitted("text")).toEqual([["# Title"]]);
    });

    it("lets nothing reach the chat after a no, and tells the user nothing more", async () => {
      core({ core_plugin_made: { sent: true } });
      const { wrapper, says } = await open({ sending: "nothing" });
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["send"]);
      needs(wrapper)[0].answer(false);
      await flushPromises();
      expect(wrapper.emitted("text")).toBeUndefined();
      expect(invoked("core_plugin_grant")).toHaveLength(0);
      // The user just said no: no notice telling where to allow it.
      expect(toast.create).not.toHaveBeenCalledWith(expect.objectContaining({ message: expect.stringContaining("may not write") }));
    });

    // Until 2026-10-09 a tool outside a conversation had no chat to write in, so nothing was asked.
    // Now the user picks who it is for (Ioan), so it asks to write in the chat first, then who to.
    it("asks to write in the chat from outside a conversation too, then who it is for", async () => {
      core();
      const { wrapper, says } = await open({ contact: "", sending: "nothing" });
      says({ type: "ft.text", text: "# Title" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["send"]);
      expect(wrapper.emitted("pick")).toBeUndefined();
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(wrapper.emitted("pick")).toHaveLength(1);
      expect(wrapper.emitted("text")).toBeUndefined();
    });

    it("asks for the live channel, then carries what the plugin says both ways", async () => {
      core({ core_plugin_live_send: true });
      const { wrapper, post, says } = await open({ live: false });
      says({ type: "ft.liveSend", id: "q1", data: "AQID" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["live"]);
      expect(invoked("core_plugin_live_send")).toHaveLength(0);
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_grant", { plugin: plugin.id, granted: expect.objectContaining({ live: true }) });
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_live_send", { plugin: plugin.id, contact: "ft_bob", data: "AQID" });
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: true }, "*");
    });

    it("answers false to the live channel after a no", async () => {
      core({ core_plugin_live_send: true });
      const { wrapper, post, says } = await open({ live: false });
      says({ type: "ft.liveSend", id: "q1", data: "AQID" });
      await flushPromises();
      needs(wrapper)[0].answer(false);
      await flushPromises();
      expect(invoked("core_plugin_live_send")).toHaveLength(0);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: false }, "*");
    });

    it("asks for reminders, the drive and an address the plugin lacks", async () => {
      const kept = core({
        core_plugin_may_use_drive: () => Boolean(kept.granted().drive),
        core_vault_status: { state: "ready" },
        core_plugin_fetch: { status: 200, body: "" },
      });
      const { wrapper, post, says } = await open();
      says({ type: "ft.remindSet", id: "r1", reminder: "milk", at: 5, text: "milk" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["remind"]);
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(invoked("core_remind_set")).toHaveLength(1);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "r1", answer: true }, "*");

      says({ type: "ft.drive", id: "d1", op: "status" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["remind", "drive"]);
      needs(wrapper)[1].answer(true);
      await flushPromises();
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: { state: "ready" } }, "*");

      says({ type: "ft.fetch", id: "f1", url: "https://api.example.com/v1", method: "GET", headers: [], body: null });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["remind", "drive", "network:api.example.com"]);
      needs(wrapper)[2].answer(true);
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_grant", { plugin: plugin.id, granted: expect.objectContaining({ network: ["api.example.com"] }) });
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "f1", answer: { status: 200, body: "" } }, "*");
      expect(kept.granted()).toMatchObject({ remind: true, drive: true, network: ["api.example.com"], location: false });
    });

    it("never asks for the drive to set it up or open it", async () => {
      core({ core_plugin_may_use_drive: false });
      const { wrapper, post, says } = await open();
      says({ type: "ft.drive", id: "d1", op: "setup" });
      await flushPromises();
      expect(needs(wrapper)).toHaveLength(0);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "d1", answer: false }, "*");
    });

    it("asks for the large room only when a record would not fit in the small one", async () => {
      core({ core_plugin_record_usage: [4 * 1024 * 1024 - 2, 4 * 1024 * 1024], core_plugin_record_get: null });
      const { wrapper, post, says } = await open();
      says({ type: "ft.recordSet", id: "q1", key: "a", value: "x" });
      await flushPromises();
      expect(needs(wrapper)).toHaveLength(0);
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q1", answer: true }, "*");

      says({ type: "ft.recordSet", id: "q2", key: "b", value: "xyz" });
      await flushPromises();
      expect(needs(wrapper).map((one) => one.key)).toEqual(["storage"]);
      needs(wrapper)[0].answer(true);
      await flushPromises();
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_grant", { plugin: plugin.id, granted: expect.objectContaining({ storage: "large" }) });
      expect(tauri.invoke).toHaveBeenCalledWith("core_plugin_record_set", expect.objectContaining({ key: "b" }));
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "q2", answer: true }, "*");
    });

    it("lets a question go unanswered when the plugin closes, and grants nothing", async () => {
      core({ core_plugin_location: FIX });
      const { wrapper, post, says } = await open();
      says({ type: "ft.location", id: "l1" });
      await flushPromises();
      const [need] = needs(wrapper);
      void (wrapper.vm as unknown as { close: () => Promise<void> }).close();
      await flushPromises();
      expect(post).toHaveBeenCalledWith({ type: "ft.done", id: "l1", answer: null }, "*");
      need.answer(true);
      await flushPromises();
      expect(invoked("core_plugin_grant")).toHaveLength(0);
    });
  });
});
