import { watch } from "vue";
import type { VideoRect } from "./calls";
import { installPlugin, type ChatMessage, type PluginPermissions, type PluginView } from "./core";
import { installed, offered, refreshOffered, refreshPlugins } from "./plugins";

export const PRESENT_BOARD = "com.flickertalk.board";
export const PRESENT_DOCUMENT = "com.flickertalk.pdfviewer";
// PLUGIN_FILE_LIMIT in src-tauri/src/client.rs
export const PRESENT_FILE_LIMIT = 32 * 1024 * 1024;
export const SEND_WAIT = 15_000;

export function canPresentWith(plugin: PluginView | undefined): plugin is PluginView {
  return Boolean(plugin?.asks.live);
}

export function needsPresentGrant(plugin: PluginView): boolean {
  return Boolean(plugin.asks.live) && !plugin.granted.live;
}

export function presentGrant(plugin: PluginView): PluginPermissions {
  return { ...plugin.granted, live: Boolean(plugin.asks.live) };
}

// not by name, the core cleans file names (safe_file_name)
export function sentFile(messages: readonly ChatMessage[], before: ReadonlySet<string>, bytes: number): string | undefined {
  return messages.find((one) => one.mine && one.kind === "file" && one.file?.bytes === bytes && !before.has(one.id))?.id;
}

export function waitFor<T>(read: () => T | undefined, limit: number): Promise<T | undefined> {
  return new Promise((resolve) => {
    let stop: (() => void) | undefined;
    const timer = setTimeout(() => {
      stop?.();
      resolve(undefined);
    }, limit);
    stop = watch(
      read,
      (value) => {
        if (value === undefined) return;
        clearTimeout(timer);
        queueMicrotask(() => stop?.());
        resolve(value);
      },
      { immediate: true },
    );
  });
}

export function holeClip(area: VideoRect | null, hole: VideoRect | null): string | undefined {
  if (!area || !hole || hole.width <= 0 || hole.height <= 0) return undefined;
  const x1 = Math.round(hole.x - area.x);
  const y1 = Math.round(hole.y - area.y);
  const x2 = x1 + Math.round(hole.width);
  const y2 = y1 + Math.round(hole.height);
  return `polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, ${x1}px ${y1}px, ${x2}px ${y1}px, ${x2}px ${y2}px, ${x1}px ${y2}px, ${x1}px ${y1}px)`;
}

export type AddedTool = "here" | "added" | "missing" | "failed";

export async function installOffered(tool: string): Promise<AddedTool> {
  if (installed.value.some((one) => one.id === tool)) return "here";
  const offer = await refreshOffered().catch(() => offered.value);
  if (!(offer ?? []).some((one) => one.id === tool)) return "missing";
  try {
    await installPlugin(tool);
    await refreshPlugins();
  } catch {
    return "failed";
  }
  return installed.value.some((one) => one.id === tool) ? "added" : "failed";
}
