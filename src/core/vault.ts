import { invoke } from "@tauri-apps/api/core";
import type { PickedFile } from "./files";

// tokens, the vault key and the recovery code stay in the core

export const VAULT_EVENT = "ft://vault";
export const VAULT_PROGRESS_EVENT = "ft://vault-progress";

export interface VaultQuota {
  used: number;
  total: number;
}

export interface DriveStatus {
  files: number;
  folders: number;
  used: number;
  pending: number;
  quota: VaultQuota | null;
  backupAt: number | null;
}

export type VaultState = "none" | "empty" | "locked" | "outdated" | "ready";

export interface VaultStatus {
  state: VaultState;
  provider: string | null;
  drive: DriveStatus | null;
  problem: string | null;
  triesLeft?: number;
  retryAt?: number | null;
}

export interface DriveFolder {
  id: string;
  name: string;
  parent: string | null;
  modified: number;
}

export interface DriveFile {
  id: string;
  name: string;
  parent: string | null;
  size: number;
  mime: string;
  modified: number;
}

export interface DrivePending {
  blob: string;
  name: string;
  parent: string | null;
  size: number;
  mime: string;
  error: string;
}

export interface DriveListing {
  folders: DriveFolder[];
  files: DriveFile[];
  pending: DrivePending[];
}

export interface BackupInfo {
  at: number;
  files: number;
  dbSize: number;
}

export async function vaultStatus(): Promise<VaultStatus> {
  return invoke<VaultStatus>("core_vault_status");
}

// the app never sees the tokens
export async function vaultConnect(provider = "google"): Promise<VaultStatus> {
  return invoke<VaultStatus>("core_vault_connect", { provider });
}

// the phrase is kept nowhere
export async function vaultSetup(phrase: string): Promise<void> {
  await invoke("core_vault_setup", { phrase });
}

export async function vaultUnlock(phrase: string): Promise<void> {
  await invoke("core_vault_unlock", { phrase });
}

export async function vaultSuggestPhrase(): Promise<string> {
  return invoke<string>("core_vault_suggest_phrase");
}

export async function vaultChangePhrase(phrase: string): Promise<void> {
  await invoke("core_vault_change_phrase", { phrase });
}

// counted as the core counts them
export const PHRASE_MIN = 12;
export const PHRASE_MAX = 100;

export async function vaultDisconnect(): Promise<void> {
  await invoke("core_vault_disconnect");
}

export async function vaultList(parent: string | null): Promise<DriveListing> {
  return invoke<DriveListing>("core_vault_list", { parent });
}

export async function vaultMkdir(name: string, parent: string | null): Promise<string> {
  return invoke<string>("core_vault_mkdir", { name, parent });
}

export async function vaultRename(id: string, name: string): Promise<void> {
  await invoke("core_vault_rename", { id, name });
}

export async function vaultMove(id: string, parent: string | null): Promise<void> {
  await invoke("core_vault_move", { id, parent });
}

export async function vaultRemove(id: string): Promise<void> {
  await invoke("core_vault_remove", { id });
}

export async function vaultUploadPicked(parent: string | null): Promise<number> {
  return invoke<number>("core_vault_upload_picked", { parent });
}

export async function vaultUploadMessage(message: string, parent: string | null): Promise<string | null> {
  return invoke<string | null>("core_vault_upload_message", { message, parent });
}

export async function vaultRetry(): Promise<number> {
  return invoke<number>("core_vault_retry");
}

export async function vaultCancelPending(blob: string): Promise<void> {
  await invoke("core_vault_cancel_pending", { blob });
}

export async function vaultDownload(id: string): Promise<PickedFile> {
  return invoke<PickedFile>("core_vault_download", { id });
}

export async function vaultOpen(id: string): Promise<void> {
  await invoke("core_vault_open", { id });
}

export async function vaultSave(id: string): Promise<void> {
  await invoke("core_vault_save", { id });
}

export async function vaultSend(id: string, contact: string): Promise<string> {
  return invoke<string>("core_vault_send", { id, contact });
}

export async function vaultBackup(): Promise<BackupInfo> {
  return invoke<BackupInfo>("core_vault_backup");
}

export async function vaultBackupInfo(): Promise<BackupInfo | null> {
  return invoke<BackupInfo | null>("core_vault_backup_info");
}

export async function vaultRestore(): Promise<BackupInfo> {
  return invoke<BackupInfo>("core_vault_restore");
}

export async function pluginMayUseDrive(plugin: string): Promise<boolean> {
  return invoke<boolean>("core_plugin_may_use_drive", { plugin }).catch(() => false);
}
