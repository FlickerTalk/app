import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { effectScope, nextTick, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import { jumpTo, useChatSearch } from "./chatSearch";

function searchIn(chatId = ref("c1"), asked = ref(false), active = ref(true)) {
  const field = { contains: vi.fn(() => false), setFocus: vi.fn(async () => {}), componentOnReady: vi.fn(async () => undefined) };
  const scope = effectScope();
  const search = scope.run(() =>
    useChatSearch({
      chatId: () => chatId.value,
      asked: () => asked.value,
      active: () => active.value,
      field: () => field as unknown as HTMLElement,
    }),
  )!;
  return { search, field, chatId, asked, active, stop: () => scope.stop() };
}

const typed = async (search: ReturnType<typeof useChatSearch>, text: string) => {
  search.typeSearch(text);
  await nextTick();
  vi.advanceTimersByTime(300);
  await flushPromises();
};

describe("jumpTo", () => {
  it("scrolls the message into the middle of the thread", () => {
    const target = document.createElement("div");
    target.dataset.message = "m3";
    target.scrollIntoView = vi.fn();
    document.body.append(target);
    jumpTo("m3");
    expect(target.scrollIntoView).toHaveBeenCalledWith({ block: "center", behavior: "smooth" });
    target.remove();
  });
});

describe("useChatSearch", () => {
  beforeEach(() => {
    seed();
    vi.useFakeTimers();
  });
  afterEach(() => vi.useRealTimers());

  it("opens empty when the page asks and focuses the field", async () => {
    const { search, field, stop } = searchIn(ref("c1"), ref(true));
    expect(search.searching.value).toBe(true);
    expect(search.query.value).toBe("");
    await flushPromises();
    expect(field.setFocus).toHaveBeenCalled();
    stop();
  });

  it("asks the core a moment after the last key and lists what it found", async () => {
    const { search, stop } = searchIn();
    search.openSearch();
    search.typeSearch("TA");
    await nextTick();
    vi.advanceTimersByTime(100);
    await typed(search, "TABLE");
    expect(calls.filter(([command]) => command === "core_search")).toEqual([["core_search", { contact: "c1", query: "TABLE" }]]);
    expect(search.hits.value.map((hit) => hit.id)).toEqual(["m3"]);
    expect(search.searched.value).toBe(true);
    stop();
  });

  it("forgets the hits when the field is emptied, and on another conversation", async () => {
    const { search, chatId, stop } = searchIn();
    search.openSearch();
    await typed(search, "table");
    await typed(search, "  ");
    expect(search.hits.value).toEqual([]);
    expect(search.searched.value).toBe(false);

    await typed(search, "table");
    chatId.value = "c2";
    await nextTick();
    expect(search.searching.value).toBe(false);
    expect(search.query.value).toBe("");
    expect(search.hits.value).toEqual([]);
    stop();
  });

  it("lights a visited hit for a moment", async () => {
    const { search, stop } = searchIn();
    search.visitHit("m3");
    expect(search.lit.value).toBe("m3");
    vi.advanceTimersByTime(1600);
    expect(search.lit.value).toBe("");
    stop();
  });

  it("focuses the field once the page is on screen when it opened behind another", async () => {
    const { search, field, active, stop } = searchIn(ref("c1"), ref(false), ref(false));
    search.openSearch();
    await flushPromises();
    expect(field.setFocus).toHaveBeenCalledTimes(1);
    active.value = true;
    await flushPromises();
    expect(field.setFocus).toHaveBeenCalledTimes(2);
    stop();
  });
});
