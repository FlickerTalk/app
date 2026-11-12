// the plugin's full-screen view has no cancel button, so the app draws the frame and the way out
import { reactive } from "vue";
import {
  cancel,
  checkPermissions,
  Format,
  openAppSettings,
  requestPermissions,
  scan,
} from "@tauri-apps/plugin-barcode-scanner";

export const scanner = reactive({ active: false, refused: null as null | "denied" | "blocked" });

const SCANNING_CLASS = "ft-scanning";

function show(active: boolean) {
  scanner.active = active;
  document.documentElement.classList.toggle(SCANNING_CLASS, active);
}

// on Android the plugin's scan throws instead of asking for the camera
async function cameraAllowed(): Promise<boolean> {
  try {
    let state = await checkPermissions();
    if (state !== "granted") state = await requestPermissions();
    if (state === "granted") return true;
    scanner.refused = (await checkPermissions()) === "denied" ? "blocked" : "denied";
  } catch {
    // no scanner on desktop
  }
  return false;
}

let giveUp: (() => void) | null = null;

export async function scanQr(): Promise<string | null> {
  scanner.refused = null;
  if (!(await cameraAllowed())) return null;
  show(true);
  const cancelled = new Promise<null>((resolve) => {
    giveUp = () => resolve(null);
  });
  try {
    // on Android the plugin (2.4.6, 2.5.0) drops the pending scan before rejecting it, so scan never settles after cancel
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

export function dismissRefusal(): void {
  scanner.refused = null;
}

export async function openCameraSettings(): Promise<void> {
  scanner.refused = null;
  await openAppSettings().catch(() => undefined);
}
