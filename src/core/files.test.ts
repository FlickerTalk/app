import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  acceptFile,
  AUTO_DOWNLOAD_CHOICES,
  autoDownloadChoice,
  fileBelongsTo,
  openFile,
  pickFiles,
  readMessageFile,
  saveFile,
  sendFile,
  sendPicked,
  setAutoDownload,
  takePhoto,
} from "./files";
import { store } from "./store";

const MiB = 1024 * 1024;

describe("files", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("offers always asking, three round sizes and never asking", () => {
    expect(AUTO_DOWNLOAD_CHOICES).toEqual([0, 10_000_000, 100_000_000, 1_000_000_000, Number.MAX_SAFE_INTEGER]);
  });

  it("takes a limit saved in binary sizes as the nearest choice", () => {
    expect([5 * MiB, 50 * MiB, 2048 * MiB].map(autoDownloadChoice)).toEqual([10_000_000, 100_000_000, 1_000_000_000]);
  });

  it("sets the download limit through the core and remembers it", async () => {
    await setAutoDownload(100_000_000);
    expect(tauri.invoke.mock.calls).toEqual([["core_set_auto_download", { bytes: 100_000_000 }]]);
    expect(store.me.autoDownload).toBe(100_000_000);
  });

  it("keeps the old limit when the core refuses the new one", async () => {
    store.me.autoDownload = 10_000_000;
    tauri.invoke.mockRejectedValueOnce(new Error("disk"));
    await expect(setAutoDownload(0)).rejects.toThrow();
    expect(store.me.autoDownload).toBe(10_000_000);
  });

  it("picks files and photos through the system and sends them by path", async () => {
    const picked = [{ path: "/data/uploads/a.jpg", name: "a.jpg", mime: "image/jpeg", size: 12 }];
    tauri.invoke.mockResolvedValue(picked);
    expect(await pickFiles()).toEqual(picked);
    await pickFiles("image/*");
    expect(await takePhoto()).toEqual(picked);
    await sendPicked("ft_bob", picked[0]);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_pick_files", { accept: "" }],
      ["core_pick_files", { accept: "image/*" }],
      ["core_take_photo"],
      ["core_send_picked", { contact: "ft_bob", file: picked[0] }],
    ]);
  });

  it("sends a file in slices of half a megabyte that add up to the whole", async () => {
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_upload_start" ? "up1" : undefined));
    const bytes = new Uint8Array(512 * 1024 + 3).map((_, index) => index % 251);
    await sendFile("ft_bob", new File([bytes], "a.bin", { type: "application/x-test" }));
    const slices = tauri.invoke.mock.calls
      .filter(([command]) => command === "core_upload_append")
      .map(([, args]) => (args as { upload: string; data: string }))
      .map(({ upload, data }) => [upload, atob(data)]);
    expect(slices.map(([upload, data]) => [upload, data.length])).toEqual([
      ["up1", 512 * 1024],
      ["up1", 3],
    ]);
    expect(Uint8Array.from(slices.map(([, data]) => data).join(""), (char) => char.charCodeAt(0))).toEqual(bytes);
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_send_file", { contact: "ft_bob", upload: "up1", name: "a.bin", mime: "application/x-test" });
  });

  it("sends an empty file of no type as plain bytes, with no slice", async () => {
    tauri.invoke.mockImplementation((command: string) => Promise.resolve(command === "core_upload_start" ? "up2" : undefined));
    await sendFile("ft_bob", new File([], "empty"));
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_upload_start"],
      ["core_send_file", { contact: "ft_bob", upload: "up2", name: "empty", mime: "application/octet-stream" }],
    ]);
  });

  it("downloads, opens, saves, reads and checks a message's file through the core", async () => {
    await acceptFile("m1");
    await openFile("m1");
    await saveFile("m1");
    tauri.invoke.mockResolvedValueOnce({ name: "a.pdf", mime: "application/pdf", data: "AA==" });
    expect(await readMessageFile("m1")).toEqual({ name: "a.pdf", mime: "application/pdf", data: "AA==" });
    tauri.invoke.mockResolvedValueOnce(true);
    expect(await fileBelongsTo("m1", "ft_bob")).toBe(true);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_accept_file", { message: "m1" }],
      ["core_open_file", { message: "m1" }],
      ["core_save_file", { message: "m1" }],
      ["core_read_message_file", { message: "m1" }],
      ["core_file_belongs_to", { file: "m1", contact: "ft_bob" }],
    ]);
  });

  it("says a file belongs to nobody when the core cannot tell", async () => {
    tauri.invoke.mockRejectedValueOnce("no core yet");
    expect(await fileBelongsTo("m1", "ft_bob")).toBe(false);
  });
});
