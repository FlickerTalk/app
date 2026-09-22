/**
 * Plugins in the app (issue app#3, Plan §53–§58). A plugin runs inside an iframe served from its
 * own scheme, with the policy its permissions allow, and only ever sees the text the user hands
 * it. It never touches the app's window, its storage or its keys.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import { plugins, type PluginView } from "./core";

/** Where the frame of a plugin lives; never the app's own origin. */
export function frameUrl(id: string): string {
  return convertFileSrc(`${id}/frame.html`, "ftplugin");
}

/** The plugins the user allowed to read what they are handed. */
export async function readyPlugins(): Promise<PluginView[]> {
  return (await plugins()).filter((plugin) => plugin.granted.messages);
}

/** What the frame says back: it is ready, or how tall it has become. */
export interface FrameMessage {
  type: "ft.ready" | "ft.height";
  height?: number;
}

/** Only the frame of this plugin is listened to, and only for what it may say. */
export function fromFrame(event: MessageEvent, frame: HTMLIFrameElement | null): FrameMessage | null {
  if (!frame || event.source !== frame.contentWindow) return null;
  const data = event.data as FrameMessage | undefined;
  if (!data || (data.type !== "ft.ready" && data.type !== "ft.height")) return null;
  return data;
}
