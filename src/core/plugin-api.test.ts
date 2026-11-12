import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  pickForPlugin,
  pluginChat,
  pluginFetch,
  pluginLiveSend,
  pluginLocation,
  pluginMade,
  pluginOpenChat,
  pluginPrint,
  pluginRef,
  pluginSave,
  takePhotoForPlugin,
} from "./plugin-api";

const NOTES = "com.flickertalk.notes";

describe("what a plugin asks of the core", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("hands the core each request with the plugin that made it", async () => {
    await pickForPlugin();
    await pickForPlugin("image/*");
    await takePhotoForPlugin();
    await pluginMade(NOTES, "ft_bob", "a.png", "image/png", "AA==");
    await pluginSave("a.png", "image/png", "AA==");
    await pluginPrint(NOTES, "a.pdf", "application/pdf", "AA==");
    await pluginLocation(NOTES);
    await pluginFetch(NOTES, "https://api.example.com/", "POST", [["content-type", "text/plain"]], "hi");
    await pluginRef(NOTES, "m1");
    await pluginOpenChat(NOTES, "ref1");
    await pluginOpenChat(NOTES, "ref1", "s1");
    await pluginChat(NOTES, "ft_bob");
    await pluginLiveSend(NOTES, "ft_bob", "AA==");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_pick_for_plugin", { accept: "" }],
      ["core_pick_for_plugin", { accept: "image/*" }],
      ["core_take_photo_for_plugin"],
      ["core_plugin_made", { plugin: NOTES, contact: "ft_bob", name: "a.png", mime: "image/png", data: "AA==" }],
      ["core_plugin_save", { name: "a.png", mime: "image/png", data: "AA==" }],
      ["core_plugin_print", { plugin: NOTES, name: "a.pdf", mime: "application/pdf", data: "AA==" }],
      ["core_plugin_location", { plugin: NOTES }],
      ["core_plugin_fetch", { plugin: NOTES, url: "https://api.example.com/", method: "POST", headers: [["content-type", "text/plain"]], body: "hi" }],
      ["core_plugin_ref", { plugin: NOTES, message: "m1" }],
      ["core_plugin_open_chat", { plugin: NOTES, reference: "ref1", session: undefined }],
      ["core_plugin_open_chat", { plugin: NOTES, reference: "ref1", session: "s1" }],
      ["core_plugin_chat", { plugin: NOTES, contact: "ft_bob" }],
      ["core_plugin_live_send", { plugin: NOTES, contact: "ft_bob", data: "AA==" }],
    ]);
  });

  it("gives the plugin what the core answers", async () => {
    const staged = { path: "/data/uploads/a.png", name: "a.png", mime: "image/png", size: 1 };
    tauri.invoke.mockResolvedValueOnce({ name: "a.png", mime: "image/png", data: "AA==" });
    expect(await pickForPlugin()).toEqual({ name: "a.png", mime: "image/png", data: "AA==" });
    tauri.invoke.mockResolvedValueOnce({ sent: false, staged });
    expect(await pluginMade(NOTES, "ft_bob", "a.png", "image/png", "AA==")).toEqual({ sent: false, staged });
    tauri.invoke.mockResolvedValueOnce({ lat: 40.4, lon: -3.7, accuracy: 12, at: 1 });
    expect(await pluginLocation(NOTES)).toEqual({ lat: 40.4, lon: -3.7, accuracy: 12, at: 1 });
    tauri.invoke.mockResolvedValueOnce({ status: 200, body: "ok" });
    expect(await pluginFetch(NOTES, "https://api.example.com/", "GET", [], null)).toEqual({ status: 200, body: "ok" });
    tauri.invoke.mockResolvedValueOnce({ contact: "ft_bob", message: "m1" });
    expect(await pluginOpenChat(NOTES, "ref1")).toEqual({ contact: "ft_bob", message: "m1" });
    tauri.invoke.mockResolvedValueOnce(true);
    expect(await pluginLiveSend(NOTES, "ft_bob", "AA==")).toBe(true);
  });
});
