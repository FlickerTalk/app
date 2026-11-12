import { invoke } from "@tauri-apps/api/core";
import type { PickedFile } from "./files";

// no path reaches the WebView
export async function pickForPlugin(accept = ""): Promise<{ name: string; mime: string; data: string } | null> {
  return invoke("core_pick_for_plugin", { accept });
}

export async function takePhotoForPlugin(): Promise<{ name: string; mime: string; data: string } | null> {
  return invoke("core_take_photo_for_plugin");
}

export interface Made {
  sent: boolean;
  staged?: PickedFile;
}

export async function pluginMade(plugin: string, contact: string, name: string, mime: string, data: string): Promise<Made> {
  return invoke<Made>("core_plugin_made", { plugin, contact, name, mime, data });
}

export async function pluginSave(name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_save", { name, mime, data });
}

export async function pluginPrint(plugin: string, name: string, mime: string, data: string): Promise<void> {
  await invoke("core_plugin_print", { plugin, name, mime, data });
}

// metres, ms
export interface PluginLocation {
  lat: number;
  lon: number;
  accuracy: number;
  at: number;
}

export async function pluginLocation(plugin: string): Promise<PluginLocation | null> {
  return invoke<PluginLocation | null>("core_plugin_location", { plugin });
}

export async function pluginFetch(
  plugin: string,
  url: string,
  method: string,
  headers: [string, string][],
  body: string | null,
): Promise<{ status: number; body: string }> {
  return invoke("core_plugin_fetch", { plugin, url, method, headers, body });
}

export const PLUGIN_EVENT = "ft://plugin";

export interface PluginEvent {
  plugin: string;
  contact: string;
  // base64
  data: string;
}

// opaque, nothing of the contact in it
export async function pluginRef(plugin: string, message: string): Promise<string> {
  return invoke<string>("core_plugin_ref", { plugin, message });
}

export async function pluginOpenChat(plugin: string, reference: string, session?: string): Promise<{ contact: string; message: string } | null> {
  return invoke<{ contact: string; message: string } | null>("core_plugin_open_chat", { plugin, reference, session });
}

// stable on this phone, different for each plugin
export async function pluginChat(plugin: string, contact: string): Promise<string> {
  return invoke<string>("core_plugin_chat", { plugin, contact });
}

export async function pluginLiveSend(plugin: string, contact: string, data: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_live_send", { plugin, contact, data });
}
