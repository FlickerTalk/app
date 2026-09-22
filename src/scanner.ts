/**
 * Reading QR codes with the camera (Contact Cards §32, move invites §60). The camera sits under
 * a see-through app that draws the frame and a way out (`ScannerOverlay`): the plugin's own
 * full-screen view has no cancel button, and without a code in front the user was stuck.
 */
import { reactive } from "vue";
import { cancel, Format, scan } from "@tauri-apps/plugin-barcode-scanner";

export const scanner = reactive({ active: false });

const SCANNING_CLASS = "ft-scanning";

function show(active: boolean) {
  scanner.active = active;
  document.documentElement.classList.toggle(SCANNING_CLASS, active);
}

/** What the camera read, or `null` if cancelled (or there is no camera). */
export async function scanQr(): Promise<string | null> {
  show(true);
  try {
    const read = await scan({ windowed: true, formats: [Format.QRCode] });
    return read.content;
  } catch {
    return null;
  } finally {
    show(false);
  }
}

export async function cancelScan(): Promise<void> {
  await cancel();
}
