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
