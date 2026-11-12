import { invoke } from "@tauri-apps/api/core";
import { store } from "./store";

const UPLOAD_SLICE = 512 * 1024;

export async function acceptFile(message: string): Promise<void> {
  await invoke("core_accept_file", { message });
}

export const AUTO_DOWNLOAD_CHOICES = [0, 10_000_000, 100_000_000, 1_000_000_000, Number.MAX_SAFE_INTEGER] as const;

// limits saved before the choices were decimal map to the nearest one
export function autoDownloadChoice(bytes: number): number {
  if (AUTO_DOWNLOAD_CHOICES.includes(bytes as (typeof AUTO_DOWNLOAD_CHOICES)[number])) return bytes;
  const sizes = AUTO_DOWNLOAD_CHOICES.slice(1, -1);
  const distance = (size: number) => Math.abs(Math.log(bytes / size));
  return sizes.reduce((best, size) => (distance(size) < distance(best) ? size : best));
}

// 0 = always ask
export async function setAutoDownload(bytes: number): Promise<void> {
  await invoke("core_set_auto_download", { bytes });
  store.me.autoDownload = bytes;
}

async function toBase64(blob: Blob): Promise<string> {
  const bytes = new Uint8Array(await blob.arrayBuffer());
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
  }
  return btoa(binary);
}

export interface PickedFile {
  path: string;
  name: string;
  mime: string;
  size: number;
}

// on Android the WebView's file input opens a screen you can't leave without picking something
export async function pickFiles(accept = ""): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_pick_files", { accept });
}

export async function takePhoto(): Promise<PickedFile[]> {
  return invoke<PickedFile[]>("core_take_photo");
}

// the bytes never go through the WebView
export async function sendPicked(contact: string, file: PickedFile): Promise<void> {
  await invoke("core_send_picked", { contact, file });
}

// base64 slices, the Android IPC only carries JSON
export async function sendFile(contact: string, file: File): Promise<void> {
  const upload = await invoke<string>("core_upload_start");
  for (let offset = 0; offset < file.size; offset += UPLOAD_SLICE) {
    const data = await toBase64(file.slice(offset, offset + UPLOAD_SLICE));
    await invoke("core_upload_append", { upload, data });
  }
  await invoke("core_send_file", { contact, upload, name: file.name, mime: file.type || "application/octet-stream" });
}

export async function openFile(message: string): Promise<void> {
  await invoke("core_open_file", { message });
}

export async function saveFile(message: string): Promise<void> {
  await invoke("core_save_file", { message });
}

export async function readMessageFile(message: string): Promise<{ name: string; mime: string; data: string }> {
  return invoke("core_read_message_file", { message });
}

export async function fileBelongsTo(file: string, contact: string): Promise<boolean> {
  return invoke<boolean>("core_file_belongs_to", { file, contact }).catch(() => false);
}
