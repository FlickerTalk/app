import { beforeEach, describe, expect, it, vi } from "vitest";

type State = "granted" | "denied" | "prompt" | "prompt-with-rationale";

const plugin = vi.hoisted(() => {
  let finish: ((value: { content: string }) => void) | null = null;
  let fail: ((reason: unknown) => void) | null = null;
  return {
    // What the system says about the camera before and after asking.
    camera: { before: "granted" as string, answer: "granted" as string, after: "granted" as string },
    asked: 0,
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
    checkPermissions: vi.fn(async (): Promise<string> => ""),
    requestPermissions: vi.fn(async (): Promise<string> => ""),
    openAppSettings: vi.fn(async () => {}),
  };
});
vi.mock("@tauri-apps/plugin-barcode-scanner", () => plugin);

import mobile from "../src-tauri/capabilities/mobile.json";
import { cancelScan, dismissRefusal, openCameraSettings, scanner, scanQr } from "./scanner";

const settle = async () => {
  for (let at = 0; at < 5; at += 1) await Promise.resolve();
};

/** The camera's state as the system reports it, before asking, the answer, and after it. */
function camera(before: State, answer: State = before, after: State = answer) {
  Object.assign(plugin.camera, { before, answer, after });
}

// The camera sits under a see-through app, which draws the frame and a way out: the plugin's own
// full-screen view has none, and without a code in front the user was stuck (iOS).
describe("scanner", () => {
  beforeEach(() => {
    plugin.scan.mockClear();
    plugin.cancel.mockClear();
    plugin.openAppSettings.mockClear();
    plugin.asked = 0;
    plugin.checkPermissions.mockReset();
    plugin.checkPermissions.mockImplementation(async () => (plugin.asked ? plugin.camera.after : plugin.camera.before));
    plugin.requestPermissions.mockReset();
    plugin.requestPermissions.mockImplementation(async () => {
      plugin.asked += 1;
      return plugin.camera.answer;
    });
    camera("granted");
    dismissRefusal();
    document.documentElement.classList.remove("ft-scanning");
  });

  it("scans under a see-through app and returns what it read", async () => {
    const reading = scanQr();
    await settle();
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
    await settle();
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
    await settle();
    await cancelScan();
    expect(await reading).toBeNull();
    expect(scanner.active).toBe(false);
    expect(document.documentElement.classList.contains("ft-scanning")).toBe(false);
  });

  it("a late result from a cancelled scan does not reopen the overlay", async () => {
    plugin.cancel.mockImplementationOnce(async () => {});
    const reading = scanQr();
    await settle();
    await cancelScan();
    plugin.read("https://flickertalk.com/add#late");
    expect(await reading).toBeNull();
    expect(scanner.active).toBe(false);
  });

  describe("the camera permission", () => {
    // Fresh install of 1.2.2 on Android (2026-10-01): «Scan code» did nothing. The plugin's
    // `scan` throws on Android when the camera was never granted; it does not ask by itself.
    it("is asked for on a fresh install, before the camera opens", async () => {
      camera("prompt", "granted");
      let answer: (state: string) => void = () => {};
      plugin.requestPermissions.mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            plugin.asked += 1;
            answer = resolve;
          }),
      );
      const reading = scanQr();
      await settle();
      expect(plugin.requestPermissions).toHaveBeenCalledTimes(1);
      // The system dialog shows over the app as it is, not over a see-through one.
      expect(scanner.active).toBe(false);
      expect(plugin.scan).not.toHaveBeenCalled();

      answer("granted");
      await settle();
      expect(plugin.scan).toHaveBeenCalled();
      expect(scanner.active).toBe(true);
      plugin.read("https://flickertalk.com/add#card");
      expect(await reading).toBe("https://flickertalk.com/add#card");
      expect(scanner.refused).toBeNull();
    });

    it("is not asked for again once granted", async () => {
      camera("granted");
      const reading = scanQr();
      await settle();
      expect(plugin.requestPermissions).not.toHaveBeenCalled();
      expect(plugin.scan).toHaveBeenCalled();
      await cancelScan();
      await reading;
    });

    // Android: after a first «Don't allow» the system can still ask again.
    it("denied: no camera, and the user is told so", async () => {
      camera("prompt", "denied", "prompt-with-rationale");
      expect(await scanQr()).toBeNull();
      expect(plugin.scan).not.toHaveBeenCalled();
      expect(scanner.active).toBe(false);
      expect(scanner.refused).toBe("denied");
    });

    it("asks again after a denial the system lets it repeat", async () => {
      camera("prompt-with-rationale", "granted");
      const reading = scanQr();
      await settle();
      expect(plugin.requestPermissions).toHaveBeenCalledTimes(1);
      expect(plugin.scan).toHaveBeenCalled();
      await cancelScan();
      await reading;
    });

    // «Don't ask again» on Android, or any denial on iOS: only the system settings can turn the
    // camera back on, so that is what the user is offered.
    it("denied for good: the user is pointed to the system settings", async () => {
      camera("denied");
      expect(await scanQr()).toBeNull();
      expect(plugin.scan).not.toHaveBeenCalled();
      expect(scanner.refused).toBe("blocked");

      await openCameraSettings();
      expect(plugin.openAppSettings).toHaveBeenCalled();
      expect(scanner.refused).toBeNull();
    });

    it("a new try forgets the last refusal", async () => {
      camera("prompt", "denied", "prompt-with-rationale");
      await scanQr();
      expect(scanner.refused).toBe("denied");
      camera("granted");
      const reading = scanQr();
      expect(scanner.refused).toBeNull();
      await settle();
      await cancelScan();
      await reading;
    });

    // Desktop, or no plugin at all: nothing to read with and nothing to tell.
    it("without a scanner there is no camera, quietly", async () => {
      plugin.checkPermissions.mockRejectedValue(new Error("unknown plugin"));
      expect(await scanQr()).toBeNull();
      expect(plugin.scan).not.toHaveBeenCalled();
      expect(scanner.active).toBe(false);
      expect(scanner.refused).toBeNull();
    });
  });
});

// The WebView may ask the scanner plugin for what this module uses, and nothing else (no
// vibration): without `check`/`request` permissions the camera could never be asked for.
describe("the scanner's capability", () => {
  it("grants exactly the commands the app uses", () => {
    const granted = mobile.permissions
      .filter((permission): permission is string => typeof permission === "string")
      .filter((permission) => permission.startsWith("barcode-scanner:"))
      .sort();
    expect(granted).toEqual(
      [
        "barcode-scanner:allow-cancel",
        "barcode-scanner:allow-check-permissions",
        "barcode-scanner:allow-open-app-settings",
        "barcode-scanner:allow-request-permissions",
        "barcode-scanner:allow-scan",
      ].sort(),
    );
    expect(mobile.platforms).toEqual(expect.arrayContaining(["android", "iOS"]));
  });
});
