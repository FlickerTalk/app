/**
 * Reading QR codes with the camera (Contact Cards §32, move invites §60). The camera sits under
 * a see-through app that draws the frame and a way out (`ScannerOverlay`): the plugin's own
 * full-screen view has no cancel button, and without a code in front the user was stuck.
 */
import { reactive } from "vue";
import {
  cancel,
  checkPermissions,
  Format,
  openAppSettings,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

/**
 * `refused` says why the camera did not open: `denied` (the system may ask again next time) or
 * `blocked` (only the system settings can turn it back on).
 */
export const scanner = reactive({ active: false, refused: null as null | "denied" | "blocked" });

const SCANNING_CLASS = "ft-scanning";

function show(active: boolean) {
  scanner.active = active;
  document.documentElement.classList.toggle(SCANNING_CLASS, active);
}

/**
 * Asks for the camera when it is not granted yet. The plugin's `scan` never asks on Android: it
 * throws, and «Scan code» did nothing on a fresh install (1.2.2, 2026-10-01). On a refusal the
 * state read again tells a denial the system may repeat (`prompt-with-rationale` on Android) from
 * one for good (`denied`: «Don't ask again» on Android, any denial on iOS).
 */
async function cameraAllowed(): Promise<boolean> {
  try {
    let state = await checkPermissions();
    if (state !== "granted") state = await requestPermissions();
    if (state === "granted") return true;
    scanner.refused = (await checkPermissions()) === "denied" ? "blocked" : "denied";
  } catch {
    // No scanner here (desktop): there is no camera to read with.
  }
  return false;
}

/** Ends the scan in progress with `null`; set while one is open. */
let giveUp: (() => void) | null = null;

/** What the camera read, or `null` if cancelled, refused (see `scanner.refused`) or there is no camera. */
export async function scanQr(): Promise<string | null> {
  scanner.refused = null;
  if (!(await cameraAllowed())) return null;
  show(true);
  const cancelled = new Promise<null>((resolve) => {
    giveUp = () => resolve(null);
  });
  try {
    // The cancel wins by itself: on Android the plugin (2.4.6, 2.5.0) drops the pending scan
    // before rejecting it, so `scan` never settles after `cancel`.
    const read = await Promise.race([scan({ windowed: true, formats: [Format.QRCode] }), cancelled]);
    return read?.content ?? null;
  } catch {
    return null;
  } finally {
    giveUp = null;
    show(false);
  }
}

export async function cancelScan(): Promise<void> {
  giveUp?.();
  await cancel();
}

/** Puts away the notice of a refused camera. */
export function dismissRefusal(): void {
  scanner.refused = null;
}

/** The app's page in the system settings, where a camera refused for good is turned back on. */
export async function openCameraSettings(): Promise<void> {
  scanner.refused = null;
  await openAppSettings().catch(() => undefined);
}
