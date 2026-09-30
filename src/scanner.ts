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

/** Ends the scan in progress with `null`; set while one is open. */
let giveUp: (() => void) | null = null;

/** What the camera read, or `null` if cancelled (or there is no camera). */
export async function scanQr(): Promise<string | null> {
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
