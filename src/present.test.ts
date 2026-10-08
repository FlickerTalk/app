import { describe, expect, it, vi } from "vitest";
import { ref } from "vue";
import type { ChatMessage, PluginView } from "./core";
import { PRESENT_BOARD, PRESENT_DOCUMENT, PRESENT_FILE_LIMIT, canPresentWith, holeClip, needsPresentGrant, presentGrant, sentFile, waitFor } from "./present";

const tool = (asks: Partial<PluginView["asks"]>, granted: Partial<PluginView["granted"]>): PluginView => ({
  id: PRESENT_BOARD,
  name: "Board",
  version: "1.1.0",
  asks: { network: [], messages: false, send: "propose", ...asks },
  granted: { network: [], messages: false, send: "nothing", ...granted },
  installedAt: 1,
});
const file = (id: string, name: string, mine = true): ChatMessage => ({
  id, mine, text: "", time: "", sentAt: 1, kind: "file",
  file: { name, size: "1 MB", mime: "application/pdf", progress: 0, state: "sending" },
});

describe("presenting in a call", () => {
  it("names the tools that present, and the biggest file a plugin is handed", () => {
    expect(PRESENT_BOARD).toBe("com.flickertalk.board");
    expect(PRESENT_DOCUMENT).toBe("com.flickertalk.pdfviewer");
    expect(PRESENT_FILE_LIMIT).toBe(32 * 1024 * 1024);
  });

  it("presents only with a tool that asks to talk to its twin", () => {
    expect(canPresentWith(undefined)).toBe(false);
    expect(canPresentWith(tool({}, {}))).toBe(false);
    expect(canPresentWith(tool({ live: true }, {}))).toBe(true);
  });

  it("asks for the live channel until it is granted", () => {
    expect(needsPresentGrant(tool({ live: true }, {}))).toBe(true);
    expect(needsPresentGrant(tool({ live: true }, { live: true }))).toBe(false);
  });

  it("grants the live channel and leaves everything else as the user set it", () => {
    const board = tool({ live: true, storage: "large" }, { send: "propose", storage: "large", location: true });
    expect(presentGrant(board)).toEqual({ network: [], messages: false, send: "propose", storage: "large", location: true, live: true });
  });

  it("finds the file I just sent by its name, among the messages that were not there before", () => {
    const before = new Set(["old"]);
    const messages = [file("old", "class.pdf"), file("theirs", "class.pdf", false), file("new", "class.pdf")];
    expect(sentFile(messages, before, "class.pdf")).toBe("new");
    expect(sentFile(messages, before, "other.pdf")).toBeUndefined();
  });

  it("waits for a value to show up, and gives up after the limit", async () => {
    const value = ref<string | undefined>();
    const found = waitFor(() => value.value, 1000);
    value.value = "m1";
    await expect(found).resolves.toBe("m1");
    vi.useFakeTimers();
    try {
      const never = waitFor(() => undefined as string | undefined, 1000);
      vi.advanceTimersByTime(1000);
      await expect(never).resolves.toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });

  // The native picture is under the WebView: the presentation must leave it a hole to show through.
  it("cuts a hole where their picture is, in the presentation's own pixels", () => {
    const area = { x: 0, y: 60, width: 400, height: 600 };
    expect(holeClip(area, { x: 290, y: 500, width: 96, height: 140 })).toBe(
      "polygon(evenodd, 0 0, 100% 0, 100% 100%, 0 100%, 0 0, 290px 440px, 386px 440px, 386px 580px, 290px 580px, 290px 440px)",
    );
    expect(holeClip(null, area)).toBeUndefined();
    expect(holeClip(area, { x: 0, y: 0, width: 0, height: 0 })).toBeUndefined();
  });
});
