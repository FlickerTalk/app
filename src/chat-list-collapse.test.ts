import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mount, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h, nextTick, ref } from "vue";
import { useChatListCollapse } from "./chat-list-collapse";
import { isChatListCollapsed, setChatListCollapsed } from "./preferences";

const marked = () => document.documentElement.classList.contains("ft-list-collapsed");

let mounted: VueWrapper | undefined;

function list(wide: boolean) {
  const width = ref(wide);
  const markedAtRender: boolean[] = [];
  let state!: ReturnType<typeof useChatListCollapse>;
  mounted = mount(
    defineComponent({
      setup() {
        state = useChatListCollapse(width);
        return () => {
          markedAtRender.push(marked());
          return h("div");
        };
      },
    }),
  );
  return { width, state, markedAtRender, wrapper: mounted };
}

describe("useChatListCollapse", () => {
  beforeEach(() => localStorage.clear());
  afterEach(() => {
    mounted?.unmount();
    mounted = undefined;
    document.documentElement.classList.remove("ft-list-collapsed");
  });

  it("starts expanded, with nothing marked on the page root", () => {
    const { state } = list(true);
    expect(state.collapsed.value).toBe(false);
    expect(marked()).toBe(false);
  });

  it("starts collapsed when the device remembers it, with the root marked before the first render", () => {
    setChatListCollapsed(true);
    const { state, markedAtRender } = list(true);
    expect(state.collapsed.value).toBe(true);
    expect(markedAtRender[0]).toBe(true);
  });

  it("keeps the list expanded on a narrow screen, and collapses it once the screen is wide", async () => {
    setChatListCollapsed(true);
    const { state, width } = list(false);
    expect(state.collapsed.value).toBe(false);
    expect(marked()).toBe(false);
    width.value = true;
    await nextTick();
    expect(state.collapsed.value).toBe(true);
    expect(marked()).toBe(true);
  });

  it("collapses and expands on demand, and remembers the choice", async () => {
    const { state } = list(true);
    state.toggle();
    await nextTick();
    expect(state.collapsed.value).toBe(true);
    expect(marked()).toBe(true);
    expect(isChatListCollapsed()).toBe(true);
    state.toggle();
    await nextTick();
    expect(state.collapsed.value).toBe(false);
    expect(marked()).toBe(false);
    expect(isChatListCollapsed()).toBe(false);
  });

  it("takes the mark off the root when the list goes away, and keeps the choice", () => {
    setChatListCollapsed(true);
    const { wrapper } = list(true);
    expect(marked()).toBe(true);
    wrapper.unmount();
    mounted = undefined;
    expect(marked()).toBe(false);
    expect(isChatListCollapsed()).toBe(true);
  });
});
