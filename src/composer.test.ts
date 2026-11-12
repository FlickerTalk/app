import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { effectScope, nextTick, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import { installTauri } from "./__tests__/tauri";
import type { ChatMessage } from "./core";
import { useComposer } from "./composer";

const MINE: ChatMessage = { id: "m2", mine: true, text: "Yes! Leaving work at 5:30", time: "09:00", sentAt: 1 };

function composerOf({ chatId = ref("c1"), request = ref(false), picker = null as HTMLInputElement | null } = {}) {
  const scope = effectScope();
  const composer = scope.run(() => useComposer({ chatId: () => chatId.value, isRequest: () => request.value, picker: () => picker }))!;
  return { composer, chatId, request, stop: () => scope.stop() };
}

const typing = () => calls.filter(([command]) => command === "core_typing");

describe("useComposer", () => {
  beforeEach(() => seed());

  it("tells the contact the user is writing, but not a stranger who wrote first, nor for spaces", async () => {
    const { composer, request, stop } = composerOf();
    composer.draft.value = "   ";
    await flushPromises();
    expect(typing()).toEqual([]);
    composer.draft.value = "hi";
    await flushPromises();
    expect(typing()).toEqual([["core_typing", { contact: "c1" }]]);
    stop();

    calls.length = 0;
    request.value = true;
    const stranger = composerOf({ request });
    stranger.composer.draft.value = "hello";
    await flushPromises();
    expect(typing()).toEqual([]);
    stranger.stop();
  });

  it("drops the edit and the answer when another conversation is shown, and the draft stays", async () => {
    const { composer, chatId, stop } = composerOf();
    composer.editing.value = MINE;
    composer.replying.value = MINE;
    composer.draft.value = "kept";
    chatId.value = "c2";
    await nextTick();
    expect(composer.editing.value).toBeNull();
    expect(composer.replying.value).toBeNull();
    expect(composer.draft.value).toBe("kept");
    stop();
  });

  it("drops an edit with its words", () => {
    const { composer, stop } = composerOf();
    composer.editing.value = MINE;
    composer.draft.value = MINE.text;
    composer.cancelEdit();
    expect(composer.editing.value).toBeNull();
    expect(composer.draft.value).toBe("");
    stop();
  });

  it("adds a picked emoji at the end of the words", () => {
    const { composer, stop } = composerOf();
    composer.draft.value = "see you ";
    composer.addEmoji("🎉");
    expect(composer.draft.value).toBe("see you 🎉");
    stop();
  });

  it("sends what is picked and folds the attach menu", async () => {
    const { composer, stop } = composerOf();
    composer.attaching.value = true;
    installTauri((command, args) => {
      calls.push([command, args]);
      return command === "core_pick_files" ? [{ path: "/p/a.jpg", name: "a.jpg", mime: "image/jpeg", size: 1 }] : undefined;
    });
    await composer.pick("image/*,video/*");
    expect(composer.attaching.value).toBe(false);
    expect(calls).toContainEqual(["core_pick_files", { accept: "image/*,video/*" }]);
    expect(calls).toContainEqual(["core_send_picked", { contact: "c1", file: { path: "/p/a.jpg", name: "a.jpg", mime: "image/jpeg", size: 1 } }]);
    stop();
  });

  it("falls back to the file input where there is no native picker", async () => {
    const picker = document.createElement("input");
    picker.click = vi.fn();
    const { composer, stop } = composerOf({ picker });
    installTauri((command) => (command === "core_pick_files" ? Promise.reject(new Error("no picker")) : undefined));
    await composer.pick("");
    expect(picker.click).toHaveBeenCalled();
    stop();
  });

  it("sends the files of the file input and empties it", async () => {
    const { composer, stop } = composerOf();
    const input = document.createElement("input");
    input.type = "file";
    Object.defineProperty(input, "files", { value: [new File(["menu"], "menu.pdf", { type: "application/pdf" })] });
    await composer.attach({ target: input } as unknown as Event);
    await flushPromises();
    expect(calls).toContainEqual(["core_send_file", { contact: "c1", upload: "up1", name: "menu.pdf", mime: "application/pdf" }]);
    stop();
  });

  it("says so when no photo can be taken", async () => {
    const { composer, stop } = composerOf();
    installTauri((command) => (command === "core_take_photo" ? Promise.reject(new Error("no camera")) : undefined));
    await composer.snap();
    expect(composer.composerError.value).toBe("Can't take a photo: the camera is not available or not allowed");
    stop();
  });

  it("sends a staged file only when asked, once", async () => {
    const { composer, stop } = composerOf();
    const file = { path: "/p/x.pdf", name: "x.pdf", mime: "application/pdf", size: 1 };
    composer.staged.value = file;
    await composer.sendStaged();
    await composer.sendStaged();
    expect(composer.staged.value).toBeNull();
    expect(calls.filter(([command]) => command === "core_send_picked")).toEqual([["core_send_picked", { contact: "c1", file }]]);
    stop();
  });
});
