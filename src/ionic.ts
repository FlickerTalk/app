import type { IonicConfig } from "@ionic/core/components";

/**
 * Ionic's settings, built once the phone's texts are loaded (main.ts). On iOS Ionic writes a word
 * beside the arrow of every back button, its own English "Back" unless told otherwise; the app
 * gives it `common.back`. Android's (Material) back button is an arrow alone and stays so.
 * The language is chosen once, at start, so the word never goes stale.
 */
export function ionicConfig(back: string, ios: boolean): IonicConfig {
  return ios ? { backButtonText: back } : {};
}
