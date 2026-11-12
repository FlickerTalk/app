import { openUrl } from "@tauri-apps/plugin-opener";

// App Review 1.2 wants the terms, the privacy policy and a contact reachable from the app

export const TERMS_URL = "https://flickertalk.com/terms/";
export const PRIVACY_URL = "https://flickertalk.com/privacy/";
export const SUPPORT_ADDRESS = "info@flickertalk.com";

export async function openOutside(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch {
    // no browser or mail app, nothing else to do
  }
}

export function mailSupport(): Promise<void> {
  return openOutside(`mailto:${SUPPORT_ADDRESS}`);
}
