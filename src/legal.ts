import { openUrl } from "@tauri-apps/plugin-opener";

// App Review guideline 1.2 (2026-10-28): the app links to the terms the user accepts by tapping
// Start, to the privacy policy and to a person who answers. The texts live on the website; the app
// only opens them, in the phone's browser or mail app, when the user asks.

/** The Terms of Use (the EULA), on the website. */
export const TERMS_URL = "https://flickertalk.com/terms/";
/** The Privacy Policy, on the website. */
export const PRIVACY_URL = "https://flickertalk.com/privacy/";
/** Where a person answers: support and questions, the same address the reports go to. */
export const SUPPORT_ADDRESS = "info@flickertalk.com";

/** Opens an address outside the app; a phone that cannot open it leaves the screen as it was. */
export async function openOutside(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch {
    // No browser or mail app to take it: there is nothing more the app can do.
  }
}

/** Opens the phone's mail app with a new message to support. */
export function mailSupport(): Promise<void> {
  return openOutside(`mailto:${SUPPORT_ADDRESS}`);
}
