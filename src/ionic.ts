import type { IonicConfig } from "@ionic/core/components";
import { pageTransition } from "./page-transition";

// on iOS Ionic writes its own English "Back" next to the arrow unless given a text
export function ionicConfig(back: string, ios: boolean): IonicConfig {
  const config: IonicConfig = { navAnimation: pageTransition };
  return ios ? { ...config, backButtonText: back } : config;
}
