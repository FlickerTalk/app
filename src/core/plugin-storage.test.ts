import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  fromBase64,
  pluginForget,
  pluginRead,
  pluginRecordForget,
  pluginRecordGet,
  pluginRecordKeys,
  pluginRecordSet,
  pluginRecordUsage,
  pluginWrite,
  toBase64Text,
} from "./plugin-storage";

const NOTES = "com.flickertalk.notes";

describe("a plugin's storage", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("turns text into base64 of its bytes and back, whatever its length or script", () => {
    expect(toBase64Text("hi")).toBe("aGk=");
    expect(fromBase64("w7E=")).toBe("ñ");
    const long = "ñ🎉".repeat(20_000);
    expect(fromBase64(toBase64Text(long))).toBe(long);
  });

  it("reads, writes and forgets small values for each session", async () => {
    tauri.invoke.mockResolvedValueOnce("v1");
    expect(await pluginRead(NOTES, "k")).toBe("v1");
    await pluginWrite(NOTES, "k", "v2", "s1");
    await pluginForget(NOTES, "k", "s1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_plugin_read", { plugin: NOTES, key: "k", session: undefined }],
      ["core_plugin_write", { plugin: NOTES, key: "k", value: "v2", session: "s1" }],
      ["core_plugin_forget", { plugin: NOTES, key: "k", session: "s1" }],
    ]);
  });

  it("keeps records in the core as base64 and hands them back as text", async () => {
    tauri.invoke.mockResolvedValueOnce(toBase64Text("{\"a\":1}"));
    expect(await pluginRecordGet(NOTES, "note/1", "s1")).toBe("{\"a\":1}");
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await pluginRecordGet(NOTES, "note/2")).toBeNull();
    await pluginRecordSet(NOTES, "note/1", "ñ", "s1");
    await pluginRecordForget(NOTES, "note/1");
    tauri.invoke.mockResolvedValueOnce(["note/1"]);
    expect(await pluginRecordKeys(NOTES, "note/")).toEqual(["note/1"]);
    tauri.invoke.mockResolvedValueOnce([12, 4096]);
    expect(await pluginRecordUsage(NOTES, "s1")).toEqual({ used: 12, quota: 4096 });
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_plugin_record_get", { plugin: NOTES, key: "note/1", session: "s1" }],
      ["core_plugin_record_get", { plugin: NOTES, key: "note/2", session: undefined }],
      ["core_plugin_record_set", { plugin: NOTES, key: "note/1", value: "w7E=", session: "s1" }],
      ["core_plugin_record_forget", { plugin: NOTES, key: "note/1", session: undefined }],
      ["core_plugin_record_keys", { plugin: NOTES, prefix: "note/", session: undefined }],
      ["core_plugin_record_usage", { plugin: NOTES, session: "s1" }],
    ]);
  });
});
