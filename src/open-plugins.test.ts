import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, type PropType } from "vue";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

const toast = vi.hoisted(() => ({ create: vi.fn() }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  toastController: { create: toast.create },
}));

import { OPEN_PLUGINS_KEY, followOpenPlugin, markClosed, markOpen, takeClosedPlugins, tellClosedPlugins } from "./open-plugins";
import { NOTICE_DURATION } from "./plugins";
import { setLocale } from "./i18n";

const chess = { id: "com.flickertalk.game.chess", name: "Chess" };
const notes = { id: "com.flickertalk.notes", name: "Notes" };

const Frame = defineComponent({
  props: {
    plugin: { type: Object as PropType<{ id: string; name: string }>, required: true },
    hidden: { type: Boolean, default: false },
  },
  setup(props) {
    followOpenPlugin(
      () => props.plugin,
      () => props.hidden,
    );
    return () => null;
  },
});

const reported = () => tauri.invoke.mock.calls.filter(([command]) => command === "core_plugin_open").map(([, args]) => args);

describe("plugins open when the page went away", () => {
  beforeEach(() => {
    localStorage.clear();
    tauri.invoke.mockReset().mockResolvedValue(undefined);
    toast.create.mockReset();
    setLocale("en");
  });

  it("names a plugin left open at the next start, and only once", () => {
    markOpen(chess);
    expect(takeClosedPlugins()).toEqual(["Chess"]);
    expect(takeClosedPlugins()).toEqual([]);
  });

  it("forgets a plugin that was closed", () => {
    markOpen(chess);
    markClosed(chess.id);
    expect(takeClosedPlugins()).toEqual([]);
  });

  it("keeps a plugin open in two places until both are closed", () => {
    markOpen(chess);
    markOpen(chess);
    markClosed(chess.id);
    expect(takeClosedPlugins()).toEqual(["Chess"]);
  });

  it("names each plugin left open once", () => {
    markOpen(chess);
    markOpen(notes);
    markOpen(chess);
    expect(takeClosedPlugins()).toEqual(["Chess", "Notes"]);
  });

  it("names nothing from a marker it cannot read", () => {
    localStorage.setItem(OPEN_PLUGINS_KEY, "{not json");
    expect(takeClosedPlugins()).toEqual([]);
    localStorage.setItem(OPEN_PLUGINS_KEY, JSON.stringify([{ id: 3 }, "x", null, { id: "a", name: "A", count: 1 }]));
    expect(takeClosedPlugins()).toEqual(["A"]);
    expect(localStorage.getItem(OPEN_PLUGINS_KEY)).toBeNull();
  });

  it("remembers a frame in sight until it goes", async () => {
    const frame = mount(Frame, { props: { plugin: chess } });
    await flushPromises();
    expect(JSON.parse(localStorage.getItem(OPEN_PLUGINS_KEY) ?? "[]")).toEqual([{ ...chess, count: 1 }]);
    frame.unmount();
    expect(takeClosedPlugins()).toEqual([]);
  });

  it("never remembers a hidden frame", async () => {
    const frame = mount(Frame, { props: { plugin: chess, hidden: true } });
    await flushPromises();
    expect(localStorage.getItem(OPEN_PLUGINS_KEY)).toBeNull();
    frame.unmount();
    expect(takeClosedPlugins()).toEqual([]);
  });

  it("tells the core when the frame changes plugin or goes", async () => {
    const frame = mount(Frame, { props: { plugin: chess } });
    await flushPromises();
    await frame.setProps({ plugin: notes });
    await flushPromises();
    expect(JSON.parse(localStorage.getItem(OPEN_PLUGINS_KEY) ?? "[]")).toEqual([{ ...notes, count: 1 }]);
    frame.unmount();
    expect(reported()).toEqual([
      { plugin: chess.id, open: false },
      { plugin: notes.id, open: true },
      { plugin: notes.id, open: false },
    ]);
  });

  it("says each closed plugin in its own toast at the top, one after the other", async () => {
    const dismissals: (() => void)[] = [];
    toast.create.mockImplementation(async () => ({
      present: () => Promise.resolve(),
      onDidDismiss: () => new Promise<void>((resolve) => dismissals.push(resolve)),
    }));
    const told = tellClosedPlugins(["Chess", "Notes"]);
    await flushPromises();
    expect(toast.create).toHaveBeenCalledTimes(1);
    expect(toast.create).toHaveBeenCalledWith({ message: "Chess was closed", duration: NOTICE_DURATION, position: "top" });
    dismissals[0]();
    await flushPromises();
    expect(toast.create).toHaveBeenCalledTimes(2);
    expect(toast.create).toHaveBeenLastCalledWith({ message: "Notes was closed", duration: NOTICE_DURATION, position: "top" });
    dismissals[1]();
    await told;
  });

  it("shows nothing when nothing was open", async () => {
    await tellClosedPlugins([]);
    expect(toast.create).not.toHaveBeenCalled();
  });
});
