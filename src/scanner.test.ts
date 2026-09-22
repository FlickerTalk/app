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
});
