import { invoke } from "@tauri-apps/api/core";

// the core checks the grant, the text and the tags first, and the text never leaves the phone

export type TranslationStatus = "installed" | "downloadable" | "unsupported";

export async function translateLanguages(plugin: string): Promise<string[]> {
  return invoke<string[]>("core_translate_languages", { plugin });
}

export async function translateDetect(plugin: string, text: string): Promise<string | null> {
  return invoke<string | null>("core_translate_detect", { plugin, text });
}

export async function translateStatus(plugin: string, from: string, to: string): Promise<TranslationStatus> {
  return invoke<TranslationStatus>("core_translate_status", { plugin, from, to });
}

// iOS may show its own sheet
export async function translatePrepare(plugin: string, from: string, to: string): Promise<boolean> {
  return invoke<boolean>("core_translate_prepare", { plugin, from, to });
}

// Android only
export async function translateRemove(plugin: string, from: string, to: string): Promise<boolean> {
  return invoke<boolean>("core_translate_remove", { plugin, from, to });
}

export async function translateText(plugin: string, text: string, from: string, to: string): Promise<string | null> {
  return invoke<string | null>("core_translate_text", { plugin, text, from, to });
}
