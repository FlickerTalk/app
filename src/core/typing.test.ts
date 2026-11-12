import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import { store } from "./store";
import { heardTyping, isTyping, quietTyping, sendTyping, TYPING_EVERY, TYPING_FADE } from "./typing";

describe("typing", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });
  afterEach(() => vi.useRealTimers());

  it("shows a contact typing until it fades", () => {
    heardTyping("ft_bob");
    expect(isTyping("ft_bob")).toBe(true);
    expect(store.typing).toEqual({ ft_bob: true });
    vi.advanceTimersByTime(TYPING_FADE);
    expect(isTyping("ft_bob")).toBe(false);
    expect(store.typing).toEqual({});
  });

  it("stops showing one contact typing at once, and leaves the others", () => {
    heardTyping("ft_bob");
    heardTyping("ft_carol");
    quietTyping("ft_bob");
    expect(store.typing).toEqual({ ft_carol: true });
    vi.advanceTimersByTime(TYPING_FADE);
    expect(store.typing).toEqual({});
  });

  it("tells the core at most once every few seconds for each contact", async () => {
    await sendTyping("ft_dave");
    await sendTyping("ft_dave");
    await sendTyping("ft_erin");
    vi.advanceTimersByTime(TYPING_EVERY - 1);
    await sendTyping("ft_dave");
    vi.advanceTimersByTime(1);
    await sendTyping("ft_dave");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_typing", { contact: "ft_dave" }],
      ["core_typing", { contact: "ft_erin" }],
      ["core_typing", { contact: "ft_dave" }],
    ]);
  });

  it("never fails when the core cannot be told", async () => {
    tauri.invoke.mockRejectedValue(new Error("no core"));
    await expect(sendTyping("ft_frank")).resolves.toBeUndefined();
  });
});
