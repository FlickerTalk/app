/**
 * Presenting in a call (docs/plan-presentar-en-llamada.md, 2026-10-08): a whiteboard or a PDF the
 * other person sees while they hear you. What the call screen needs that is not on the screen:
 * which tools present, the permission they need (`live`, asked once, like a game's), the file just
 * sent to the chat, and the hole the presentation leaves for the other person's picture.
 */
import { watch } from "vue";
import type { VideoRect } from "./calls";
import type { ChatMessage, PluginPermissions, PluginView } from "./core";

export const PRESENT_BOARD = "com.flickertalk.board";
export const PRESENT_DOCUMENT = "com.flickertalk.pdfviewer";
/** The most a plugin is handed (`PLUGIN_FILE_LIMIT` in src-tauri/src/client.rs). */
export const PRESENT_FILE_LIMIT = 32 * 1024 * 1024;
/** How long a sent PDF may take to show up in the chat before presenting gives up. */
export const SEND_WAIT = 15_000;

/** A tool that can present: here, and asking to talk to its twin (an older version does not). */
export function canPresentWith(plugin: PluginView | undefined): plugin is PluginView {
  return Boolean(plugin?.asks.live);
}

/** Whether the tool still lacks the live channel the presentation goes through. */
export function needsPresentGrant(plugin: PluginView): boolean {
  return Boolean(plugin.asks.live) && !plugin.granted.live;
}

/** What one "allow" grants: the live channel, never more than asked; the rest stays as it was. */
export function presentGrant(plugin: PluginView): PluginPermissions {
  return { ...plugin.granted, live: Boolean(plugin.asks.live) };
}

/** The id of the file I just sent: mine, of this many bytes, and not among the messages before.
 *  Never by its name: the core keeps a cleaned one (`safe_file_name`, "a: b.pdf" → "a_ b.pdf"). */
export function sentFile(messages: readonly ChatMessage[], before: ReadonlySet<string>, bytes: number): string | undefined {
  return messages.find((one) => one.mine && one.kind === "file" && one.file?.bytes === bytes && !before.has(one.id))?.id;
}

/** The first value `read` gives that is not undefined, or undefined after `limit` ms. */
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

/** A `clip-path` for the presentation with a hole where the picture is, or none without one. */
export function holeClip(area: VideoRect | null, hole: VideoRect | null): string | undefined {
  if (!area || !hole || hole.width <= 0 || hole.height <= 0) return undefined;
  const x1 = Math.round(hole.x - area.x);
  const y1 = Math.round(hole.y - area.y);
  const x2 = x1 + Math.round(hole.width);
  const y2 = y1 + Math.round(hole.height);
  return `polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, ${x1}px ${y1}px, ${x2}px ${y1}px, ${x2}px ${y2}px, ${x1}px ${y2}px, ${x1}px ${y1}px)`;
}
