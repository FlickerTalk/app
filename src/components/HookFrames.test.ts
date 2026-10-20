import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { enableAutoUnmount, flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, reactive, ref } from "vue";
import HookFrames from "./HookFrames.vue";
import { installed } from "../plugins";
import type { PluginView } from "../core";

// 2026-10-20 (plan of the translator §8.1): the frames that run a conversation's hooks out of sight,
// one per tool, shared by a conversation, the contact's page and a circle's. A frame shows (as the
// tool's window, with a way out) only while its tool asks the user something; a permission the
// tool needs is asked on the spot, in the same sheet as anywhere else, named after it.
const TRANSLATOR: PluginView = {
  id: "com.flickertalk.translate",
  name: "Translator",
  version: "1.1.0",
  kind: "tool",
  asks: { network: [], messages: true, send: "propose", translate: true },
  granted: { network: [], messages: true, send: "propose", translate: false },
  installedAt: 1,
  hooks: { message: { label: "Translate", icon: "language-outline" } },
};

const sheets: { props: Record<string, unknown>; emit: (event: string, ...args: unknown[]) => void; runHook: ReturnType<typeof vi.fn> }[] = [];
const PluginSheet = defineComponent({
  name: "PluginSheet",
  props: {
    plugin: { type: Object, default: undefined },
    contact: { type: String, default: undefined },
    session: { type: String, default: undefined },
    sending: { type: String, default: "nothing" },
    hidden: Boolean,
    fill: Boolean,
  },
  emits: ["shown", "needs", "done"],
  setup(props, { expose, emit }) {
    const runHook = vi.fn(async () => null);
    sheets.push({ props, emit: emit as (event: string, ...args: unknown[]) => void, runHook });
    expose({ runHook });
    return () => h("div", { "data-test": "hook-frame" });
  },
});
const PermissionAsk = defineComponent({
  name: "PermissionAsk",
  props: { open: Boolean, name: { type: String, default: "" }, permission: { type: Object, default: undefined } },
  emits: ["allow", "cancel"],
  setup(props, { emit }) {
    return () =>
      props.open
        ? h("div", { "data-test": "permission-ask" }, [
            h("span", props.name),
            h("button", { "data-test": "permission-allow", onClick: () => emit("allow") }),
            h("button", { "data-test": "permission-cancel", onClick: () => emit("cancel") }),
          ])
        : null;
  },
});
const stubs = { PluginSheet, PermissionAsk, IonIcon: true };

enableAutoUnmount(afterEach);

function hooksWith(ids: string[]) {
  return {
    frames: ref(ids),
    shown: reactive(new Set<string>()),
    attach: vi.fn(),
    setShown: vi.fn(),
    dismiss: vi.fn(),
  };
}

describe("HookFrames", () => {
  beforeEach(() => {
    sheets.length = 0;
    installed.value = [TRANSLATOR];
  });
  afterEach(() => (installed.value = []));

  it("mounts a frame out of sight for each tool the hooks ask for, and hands it over", async () => {
    const hooks = hooksWith([TRANSLATOR.id, "com.example.gone"]);
    const wrapper = mount(HookFrames, { props: { hooks, contact: "c1", session: "s1" }, global: { stubs } });
    await flushPromises();
    expect(sheets).toHaveLength(1);
    expect(sheets[0].props).toMatchObject({ plugin: TRANSLATOR, contact: "c1", session: "s1", sending: "propose", hidden: true });
    expect(hooks.attach).toHaveBeenCalledWith(TRANSLATOR.id, expect.objectContaining({ runHook: sheets[0].runHook }));
    const window = wrapper.find(`[data-test='hook-window-${TRANSLATOR.id}']`);
    expect(window.classes()).toContain("is-unseen");
    expect(window.attributes("aria-hidden")).toBe("true");
  });

  it("shows a frame only while its tool asks the user something, with a way out", async () => {
    const hooks = hooksWith([TRANSLATOR.id]);
    const wrapper = mount(HookFrames, { props: { hooks, contact: "c1" }, global: { stubs } });
    await flushPromises();
    sheets[0].emit("shown", true);
    expect(hooks.setShown).toHaveBeenCalledWith(TRANSLATOR.id, true);
    hooks.shown.add(TRANSLATOR.id);
    await flushPromises();
    const window = wrapper.find(`[data-test='hook-window-${TRANSLATOR.id}']`);
    expect(window.classes()).not.toContain("is-unseen");
    expect(window.attributes("role")).toBe("dialog");
    expect(window.attributes("aria-label")).toBe("Translator");
    await wrapper.find("[data-test='close-hook']").trigger("click");
    expect(hooks.dismiss).toHaveBeenCalledWith(TRANSLATOR.id);
  });

  it("asks on the spot for a permission the tool needs, named after it", async () => {
    const hooks = hooksWith([TRANSLATOR.id]);
    const wrapper = mount(HookFrames, { props: { hooks, contact: "c1" }, global: { stubs } });
    await flushPromises();
    const answer = vi.fn();
    sheets[0].emit("needs", { label: "Translate text on this phone", icon: "language", answer });
    await flushPromises();
    expect(wrapper.find("[data-test='permission-ask']").text()).toContain("Translator");
    await wrapper.find("[data-test='permission-allow']").trigger("click");
    expect(answer).toHaveBeenCalledWith(true);
    expect(wrapper.find("[data-test='permission-ask']").exists()).toBe(false);
  });
});
