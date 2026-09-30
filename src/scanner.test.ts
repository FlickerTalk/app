import { beforeEach, describe, expect, it, vi } from "vitest";

const plugin = vi.hoisted(() => {
  let finish: ((value: { content: string }) => void) | null = null;
  let fail: ((reason: unknown) => void) | null = null;
  return {
    scan: vi.fn(
      () =>
        new Promise<{ content: string }>((resolve, reject) => {
          finish = resolve;
          fail = reject;
        }),
    ),
    cancel: vi.fn(async () => fail?.("cancelled")),
    read: (content: string) => finish?.({ content }),
    Format: { QRCode: "QR_CODE" },
  };
});
vi.mock("@tauri-apps/plugin-barcode-scanner", () => plugin);

import { cancelScan, scanner, scanQr } from "./scanner";

// The camera sits under a see-through app, which draws the frame and a way out: the plugin's own
// full-screen view has none, and without a code in front the user was stuck (iOS).
describe("scanner", () => {
  beforeEach(() => {
    plugin.scan.mockClear();
    plugin.cancel.mockClear();
    document.documentElement.classList.remove("ft-scanning");
  });

  it("scans under a see-through app and returns what it read", async () => {
    const reading = scanQr();
    expect(plugin.scan).toHaveBeenCalledWith({ windowed: true, formats: ["QR_CODE"] });
    expect(scanner.active).toBe(true);
    expect(document.documentElement.classList.contains("ft-scanning")).toBe(true);
    plugin.read("https://flickertalk.com/add#card");
    expect(await reading).toBe("https://flickertalk.com/add#card");
    expect(scanner.active).toBe(false);
    expect(document.documentElement.classList.contains("ft-scanning")).toBe(false);
  });

  it("can be cancelled", async () => {
    const reading = scanQr();
    await cancelScan();
    expect(plugin.cancel).toHaveBeenCalled();
    expect(await reading).toBeNull();
    expect(scanner.active).toBe(false);
  });

  // tauri-plugin-barcode-scanner 2.4.6 (and 2.5.0) on Android forgets the pending scan before
  // rejecting it in `cancel`, so `scan` never settles: the camera closed but the see-through
  // overlay stayed on screen with no way out.
  it("gets out on cancel even when the plugin never settles the scan (Android)", async () => {
    plugin.cancel.mockImplementationOnce(async () => {});
    const reading = scanQr();
    await cancelScan();
    expect(await reading).toBeNull();
    expect(scanner.active).toBe(false);
    expect(document.documentElement.classList.contains("ft-scanning")).toBe(false);
  });

  it("a late result from a cancelled scan does not reopen the overlay", async () => {
    plugin.cancel.mockImplementationOnce(async () => {});
    const reading = scanQr();
    await cancelScan();
    plugin.read("https://flickertalk.com/add#late");
    expect(await reading).toBeNull();
    expect(scanner.active).toBe(false);
  });
});
