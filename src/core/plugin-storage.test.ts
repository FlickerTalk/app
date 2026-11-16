import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  answerStorage,
  asksStorage,
  fromBase64,
  gamePlayer,
  pluginForget,
  pluginRead,
  pluginRecordForget,
  pluginRecordGet,
  pluginRecordKeys,
  pluginRecordSet,
  pluginRecordUsage,
  pluginWrite,
  setGamePlayer,
  toBase64Text,
} from "./plugin-storage";

const NOTES = "com.flickertalk.notes";
const CHESS = "com.flickertalk.game.chess";

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

  it("reads and writes the player every game shares, for each session", async () => {
    const player = { who: "juno", hair: "curly-puff", hairColour: "copper", name: "Ana" };
    tauri.invoke.mockResolvedValueOnce(player);
    expect(await gamePlayer(CHESS, "s1")).toEqual(player);
    tauri.invoke.mockResolvedValueOnce(null);
    expect(await gamePlayer(CHESS)).toBeNull();
    await setGamePlayer(CHESS, player, "s1");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_game_player", { plugin: CHESS, session: "s1" }],
      ["core_game_player", { plugin: CHESS, session: undefined }],
      ["core_set_game_player", { plugin: CHESS, player, session: "s1" }],
    ]);
  });

  it("tells a frame's question about what its plugin keeps from any other question", () => {
    const kept = ["ft.read", "ft.write", "ft.forget", "ft.recordGet", "ft.recordSet", "ft.recordForget", "ft.recordKeys", "ft.recordUsage"];
    for (const type of [...kept, "ft.playerGet", "ft.playerSet"]) {
      expect(asksStorage({ type }), type).toBe(true);
    }
    for (const type of ["ft.remindSet", "ft.fetch", "ft.drive", "ft.location", "ft.liveSend"]) expect(asksStorage({ type }), type).toBe(false);
  });

  it("answers what a plugin keeps with what its frame expects back", async () => {
    tauri.invoke.mockImplementation(async (command: string) => {
      if (command === "core_plugin_read") return "black";
      if (command === "core_plugin_record_get") return toBase64Text("milk");
      if (command === "core_plugin_record_keys") return ["note/1"];
      if (command === "core_plugin_record_usage") return [4, 4096];
      return undefined;
    });
    expect(await answerStorage(NOTES, { type: "ft.read", key: "pen" }, "s1")).toBe("black");
    expect(await answerStorage(NOTES, { type: "ft.write", key: "pen", value: "red" }, "s1")).toBe(true);
    expect(await answerStorage(NOTES, { type: "ft.forget", key: "pen" })).toBe(true);
    expect(await answerStorage(NOTES, { type: "ft.recordGet", key: "note/1" })).toBe("milk");
    expect(await answerStorage(NOTES, { type: "ft.recordSet", key: "note/1", value: "ñ" }, "s1")).toBe(true);
    expect(await answerStorage(NOTES, { type: "ft.recordForget", key: "note/1" })).toBe(true);
    expect(await answerStorage(NOTES, { type: "ft.recordKeys", prefix: "note/" })).toEqual(["note/1"]);
    expect(await answerStorage(NOTES, { type: "ft.recordUsage" }, "s1")).toEqual({ used: 4, quota: 4096 });
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_plugin_read", { plugin: NOTES, key: "pen", session: "s1" }],
      ["core_plugin_write", { plugin: NOTES, key: "pen", value: "red", session: "s1" }],
      ["core_plugin_forget", { plugin: NOTES, key: "pen", session: undefined }],
      ["core_plugin_record_get", { plugin: NOTES, key: "note/1", session: undefined }],
      ["core_plugin_record_set", { plugin: NOTES, key: "note/1", value: "w7E=", session: "s1" }],
      ["core_plugin_record_forget", { plugin: NOTES, key: "note/1", session: undefined }],
      ["core_plugin_record_keys", { plugin: NOTES, prefix: "note/", session: undefined }],
      ["core_plugin_record_usage", { plugin: NOTES, session: "s1" }],
    ]);
  });

  it("answers a game's player with the record, and a kept one with true", async () => {
    const player = { who: "juno", hair: "curly-puff", hairColour: "copper", name: "Ana" };
    tauri.invoke.mockImplementation(async (command: string) => (command === "core_game_player" ? player : undefined));
    expect(await answerStorage(CHESS, { type: "ft.playerGet" }, "s1")).toEqual(player);
    expect(await answerStorage(CHESS, { type: "ft.playerSet", player }, "s1")).toBe(true);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_game_player", { plugin: CHESS, session: "s1" }],
      ["core_set_game_player", { plugin: CHESS, player, session: "s1" }],
    ]);
  });
});
