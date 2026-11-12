import { describe, expect, it, vi } from "vitest";
import { effectScope, nextTick, ref } from "vue";
import type { ChatMessage } from "./core";
import { usePinnedMessages } from "./pinnedMessages";

const message = (id: string, extra: Partial<ChatMessage> = {}): ChatMessage => ({ id, mine: false, text: `text ${id}`, time: "09:00", sentAt: 1, ...extra });

function pinsOf(list: ChatMessage[]) {
  const messages = ref(list);
  const scope = effectScope();
  const pins = scope.run(() => usePinnedMessages(() => messages.value))!;
  return { pins, messages, stop: () => scope.stop() };
}

describe("usePinnedMessages", () => {
  it("shows nothing without a pinned message", () => {
    const { pins, stop } = pinsOf([message("a"), message("b")]);
    expect(pins.pinned.value).toEqual([]);
    expect(pins.shownPin.value).toBeUndefined();
    stop();
  });

  it("shows the latest pinned first and walks through them, going to each one", () => {
    const targets = ["a", "c"].map((id) => {
      const target = document.createElement("div");
      target.dataset.message = id;
      target.scrollIntoView = vi.fn();
      document.body.append(target);
      return target;
    });
    const { pins, stop } = pinsOf([message("a", { pinned: true }), message("b"), message("c", { pinned: true })]);
    expect(pins.shownPin.value?.id).toBe("c");
    pins.visitPin();
    expect(targets[1].scrollIntoView).toHaveBeenCalled();
    expect(pins.shownPin.value?.id).toBe("a");
    expect(pins.pinnedAt.value).toBe(1);
    pins.visitPin();
    expect(pins.shownPin.value?.id).toBe("c");
    targets.forEach((target) => target.remove());
    stop();
  });

  it("stays within the list when a pin goes away", async () => {
    const { pins, messages, stop } = pinsOf([message("a", { pinned: true }), message("b", { pinned: true })]);
    pins.visitPin();
    expect(pins.pinnedAt.value).toBe(1);
    messages.value = [message("a", { pinned: true }), message("b")];
    await nextTick();
    expect(pins.pinnedAt.value).toBe(0);
    expect(pins.shownPin.value?.id).toBe("a");
    stop();
  });

  it("names a pinned file by its file name", () => {
    const { pins, stop } = pinsOf([]);
    expect(pins.pinText(message("f", { kind: "file", text: "", file: { name: "menu.pdf" } as ChatMessage["file"] }))).toBe("menu.pdf");
    expect(pins.pinText(message("t"))).toBe("text t");
    stop();
  });
});
