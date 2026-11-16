import { invoke } from "@tauri-apps/api/core";

// stored per place (session), the plugin never hears of sessions

export async function pluginRead(plugin: string, key: string, session?: string): Promise<string | null> {
  return invoke<string | null>("core_plugin_read", { plugin, key, session });
}

export async function pluginWrite(plugin: string, key: string, value: string, session?: string): Promise<void> {
  await invoke("core_plugin_write", { plugin, key, value, session });
}

export async function pluginForget(plugin: string, key: string, session?: string): Promise<void> {
  await invoke("core_plugin_forget", { plugin, key, session });
}

export async function pluginRecordGet(plugin: string, key: string, session?: string): Promise<string | null> {
  const value = await invoke<string | null>("core_plugin_record_get", { plugin, key, session });
  return value === null ? null : fromBase64(value);
}

export async function pluginRecordSet(plugin: string, key: string, value: string, session?: string): Promise<void> {
  await invoke("core_plugin_record_set", { plugin, key, value: toBase64Text(value), session });
}

export async function pluginRecordForget(plugin: string, key: string, session?: string): Promise<void> {
  await invoke("core_plugin_record_forget", { plugin, key, session });
}

export async function pluginRecordKeys(plugin: string, prefix: string, session?: string): Promise<string[]> {
  return invoke<string[]>("core_plugin_record_keys", { plugin, prefix, session });
}

// bytes
export async function pluginRecordUsage(plugin: string, session?: string): Promise<{ used: number; quota: number }> {
  const [used, quota] = await invoke<[number, number]>("core_plugin_record_usage", { plugin, session });
  return { used, quota };
}

export interface PluginPlayer {
  who: string;
  hair: string;
  hairColour: string;
  name: string;
}

export async function gamePlayer(plugin: string, session?: string): Promise<PluginPlayer | null> {
  return invoke<PluginPlayer | null>("core_game_player", { plugin, session });
}

export async function setGamePlayer(plugin: string, player: PluginPlayer, session?: string): Promise<void> {
  await invoke("core_set_game_player", { plugin, player, session });
}

export type StorageQuestion =
  | { type: "ft.read" | "ft.forget" | "ft.recordGet" | "ft.recordForget"; key: string }
  | { type: "ft.write" | "ft.recordSet"; key: string; value: string }
  | { type: "ft.recordKeys"; prefix: string }
  | { type: "ft.recordUsage" }
  | { type: "ft.playerGet" }
  | { type: "ft.playerSet"; player: PluginPlayer };

const STORAGE_QUESTIONS: ReadonlySet<string> = new Set<StorageQuestion["type"]>([
  "ft.read", "ft.write", "ft.forget", "ft.recordGet", "ft.recordSet", "ft.recordForget", "ft.recordKeys", "ft.recordUsage",
  "ft.playerGet", "ft.playerSet",
]);

export function asksStorage(said: { type: string }): said is StorageQuestion {
  return STORAGE_QUESTIONS.has(said.type);
}

export async function answerStorage(plugin: string, said: StorageQuestion, session?: string): Promise<unknown> {
  switch (said.type) {
    case "ft.read":
      return pluginRead(plugin, said.key, session);
    case "ft.write":
      await pluginWrite(plugin, said.key, said.value, session);
      return true;
    case "ft.forget":
      await pluginForget(plugin, said.key, session);
      return true;
    case "ft.recordGet":
      return pluginRecordGet(plugin, said.key, session);
    case "ft.recordSet":
      await pluginRecordSet(plugin, said.key, said.value, session);
      return true;
    case "ft.recordForget":
      await pluginRecordForget(plugin, said.key, session);
      return true;
    case "ft.recordKeys":
      return pluginRecordKeys(plugin, said.prefix, session);
    case "ft.recordUsage":
      return pluginRecordUsage(plugin, session);
    case "ft.playerGet":
      return gamePlayer(plugin, session);
    case "ft.playerSet":
      await setGamePlayer(plugin, said.player, session);
      return true;
  }
}

export function toBase64Text(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

export function fromBase64(value: string): string {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return new TextDecoder().decode(bytes);
}
