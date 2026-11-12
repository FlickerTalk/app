import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  PHRASE_MAX,
  PHRASE_MIN,
  pluginMayUseDrive,
  vaultBackup,
  vaultBackupInfo,
  vaultCancelPending,
  vaultChangePhrase,
  vaultConnect,
  vaultDisconnect,
  vaultDownload,
  vaultList,
  vaultMkdir,
  vaultMove,
  vaultOpen,
  vaultRemove,
  vaultRename,
  vaultRestore,
  vaultRetry,
  vaultSave,
  vaultSend,
  vaultSetup,
  vaultStatus,
  vaultSuggestPhrase,
  vaultUnlock,
  vaultUploadMessage,
  vaultUploadPicked,
} from "./vault";

describe("the user's own drive", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("connects to Google unless told otherwise, and hands the phrase only to the core", async () => {
    await vaultStatus();
    await vaultConnect();
    await vaultConnect("dropbox");
    await vaultSetup("correct horse battery staple");
    await vaultUnlock("correct horse battery staple");
    await vaultSuggestPhrase();
    await vaultChangePhrase("another long phrase here");
    await vaultDisconnect();
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_vault_status"],
      ["core_vault_connect", { provider: "google" }],
      ["core_vault_connect", { provider: "dropbox" }],
      ["core_vault_setup", { phrase: "correct horse battery staple" }],
      ["core_vault_unlock", { phrase: "correct horse battery staple" }],
      ["core_vault_suggest_phrase"],
      ["core_vault_change_phrase", { phrase: "another long phrase here" }],
      ["core_vault_disconnect"],
    ]);
  });

  it("counts a phrase's length within the limits the core uses", () => {
    expect([PHRASE_MIN, PHRASE_MAX]).toEqual([12, 100]);
  });

  it("hands the core every change to the drive's folders and files", async () => {
    await vaultList(null);
    await vaultList("f1");
    await vaultMkdir("Photos", null);
    await vaultRename("f1", "Pictures");
    await vaultMove("f2", "f1");
    await vaultRemove("f2");
    await vaultUploadPicked("f1");
    await vaultUploadMessage("m1", null);
    await vaultRetry();
    await vaultCancelPending("b1");
    await vaultDownload("f3");
    await vaultOpen("f3");
    await vaultSave("f3");
    await vaultSend("f3", "ft_bob");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_vault_list", { parent: null }],
      ["core_vault_list", { parent: "f1" }],
      ["core_vault_mkdir", { name: "Photos", parent: null }],
      ["core_vault_rename", { id: "f1", name: "Pictures" }],
      ["core_vault_move", { id: "f2", parent: "f1" }],
      ["core_vault_remove", { id: "f2" }],
      ["core_vault_upload_picked", { parent: "f1" }],
      ["core_vault_upload_message", { message: "m1", parent: null }],
      ["core_vault_retry"],
      ["core_vault_cancel_pending", { blob: "b1" }],
      ["core_vault_download", { id: "f3" }],
      ["core_vault_open", { id: "f3" }],
      ["core_vault_save", { id: "f3" }],
      ["core_vault_send", { id: "f3", contact: "ft_bob" }],
    ]);
  });

  it("backs up, describes the backup and restores through the core", async () => {
    const backup = { at: 1_900_000_000_000, files: 3, dbSize: 4096 };
    tauri.invoke.mockResolvedValue(backup);
    expect(await vaultBackup()).toEqual(backup);
    expect(await vaultBackupInfo()).toEqual(backup);
    expect(await vaultRestore()).toEqual(backup);
    expect(tauri.invoke.mock.calls).toEqual([["core_vault_backup"], ["core_vault_backup_info"], ["core_vault_restore"]]);
  });

  it("lets a plugin use the drive only when the core says so", async () => {
    tauri.invoke.mockResolvedValueOnce(true);
    expect(await pluginMayUseDrive("com.flickertalk.drive")).toBe(true);
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_plugin_may_use_drive", { plugin: "com.flickertalk.drive" });
    tauri.invoke.mockRejectedValueOnce(new Error("no core"));
    expect(await pluginMayUseDrive("com.flickertalk.drive")).toBe(false);
  });
});
