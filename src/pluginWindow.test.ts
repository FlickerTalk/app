import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";
import { calls, seed } from "./__tests__/seed";
import { installTauri } from "./__tests__/tauri";
import { chat, type PickedFile, type PluginView } from "./core";
import { refreshPlugins } from "./plugins";
import type { PermissionNeed } from "./permissions";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => void (back.handler === handler && (back.handler = null)) };
  },
}));

import { usePluginWindow } from "./pluginWindow";

const TOOL = { id: "com.flickertalk.code", name: "Code block", sending: "nothing" as const, live: false };
const GAME = { id: "com.flickertalk.game.chess", name: "Chess", sending: "propose" as const, live: true, game: true };

function host({ split = false, withSheet = true } = {}) {
  const chatId = ref("c1");
  const active = ref(true);
  const draft = ref("");
  const staged = ref<PickedFile | null>(null);
  const sheet = { close: vi.fn(async () => {}) };
  const scrollToEnd = vi.fn(async () => {});
  let window!: ReturnType<typeof usePluginWindow>;
  const wrapper = mount(
    defineComponent({
      setup() {
        window = usePluginWindow({
          chatId: () => chatId.value,
          active: () => active.value,
          split: () => split,
          messages: () => chat(chatId.value)?.messages ?? [],
          sheet: () => (withSheet ? sheet : null),
          gameArea: () => null,
          scrollToEnd,
          draft,
          staged,
        });
        return () => h("div");
      },
    }),
  );
  return { window, chatId, active, draft, staged, sheet, scrollToEnd, wrapper };
}

describe("usePluginWindow", () => {
  beforeEach(() => {
    seed();
    push.mockClear();
  });

  it("opens a plugin for this conversation, a new opening each time", () => {
    const { window } = host({ withSheet: false });
    window.openPlugin(TOOL);
    expect(window.plugin.value).toMatchObject({ ...TOOL, contact: "c1", opening: 1 });
    window.closePlugin();
    window.openPlugin(TOOL);
    expect(window.plugin.value?.opening).toBe(2);
  });

  it("asks the plugin to say goodbye once, and lets it go when it has", () => {
    const { window, sheet } = host();
    window.openPlugin(TOOL);
    window.closePlugin();
    window.closePlugin();
    expect(sheet.close).toHaveBeenCalledTimes(1);
    expect(window.leaving.value).toBe(true);
    expect(window.plugin.value).not.toBeNull();
    window.pluginClosed();
    expect(window.plugin.value).toBeNull();
    expect(window.leaving.value).toBe(false);
  });

  it("opens a plugin chosen while another is on screen once that one has gone", () => {
    const { window, sheet } = host();
    window.openPlugin(GAME);
    window.openPlugin(TOOL);
    expect(sheet.close).toHaveBeenCalledTimes(1);
    expect(window.plugin.value?.id).toBe(GAME.id);
    window.pluginClosed();
    expect(window.plugin.value?.id).toBe(TOOL.id);
  });

  it("keeps Back while the plugin says goodbye, only with the conversation on screen", async () => {
    const { window, active } = host();
    window.openPlugin(TOOL);
    window.closePlugin();
    await flushPromises();
    expect(back.handler).not.toBeNull();
    active.value = false;
    await flushPromises();
    expect(back.handler).toBeNull();
    active.value = true;
    await flushPromises();
    expect(back.handler).not.toBeNull();
    window.pluginClosed();
    await flushPromises();
    expect(back.handler).toBeNull();
  });

  it("keeps a game open when it proposes a text or a file, and closes a tool", () => {
    const game = host();
    game.window.openPlugin(GAME);
    game.window.fromPlugin("I won");
    expect(game.draft.value).toBe("I won");
    expect(game.window.leaving.value).toBe(false);
    game.window.stage({ path: "/p/game.pgn", name: "game.pgn", mime: "application/x-chess-pgn", size: 9 });
    game.window.pluginDone();
    expect(game.staged.value?.name).toBe("game.pgn");
    expect(game.window.leaving.value).toBe(false);
    game.window.pluginDone();
    expect(game.window.leaving.value).toBe(true);

    const tool = host();
    tool.window.openPlugin(TOOL);
    tool.window.fromPlugin("# Title");
    expect(tool.draft.value).toBe("# Title");
    expect(tool.window.leaving.value).toBe(true);
  });

  it("goes to another conversation the plugin asks for, and closes for this one", () => {
    const { window } = host();
    window.openPlugin(TOOL);
    window.openChatFromPlugin("c2");
    expect(push).toHaveBeenCalledWith("/chat/c2");
    expect(window.leaving.value).toBe(false);
    window.openChatFromPlugin("c1");
    expect(push).toHaveBeenCalledTimes(1);
    expect(window.leaving.value).toBe(true);
  });

  it("answers no to a question still open when the plugin closes", () => {
    const { window } = host();
    window.openPlugin(TOOL);
    const need = { key: "location", label: "Your location", icon: "", on: false, answer: vi.fn() } as unknown as PermissionNeed;
    window.needFromApp(need);
    expect(window.needing.value?.key).toBe("location");
    window.closePlugin();
    expect(need.answer).toHaveBeenCalledWith(false);
    expect(window.needing.value).toBeNull();
  });

  it("asks to read the messages on the spot and hands back the tool once allowed", async () => {
    const view: PluginView = {
      id: "com.flickertalk.translate",
      name: "Translator",
      version: "1.0.0",
      asks: { network: [], messages: true, send: "nothing" },
      granted: { network: [], messages: false, send: "nothing" },
      installedAt: 1,
    };
    let granted = view.granted;
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_plugin_grant") granted = args?.granted as PluginView["granted"];
      if (command === "core_plugins") return [{ ...view, granted }];
      return undefined;
    });
    await refreshPlugins();
    const { window } = host();
    const allowed = window.allowedToRead(view);
    expect(window.askingFor.value?.id).toBe(view.id);
    expect(window.needing.value?.key).toBe("messages");
    window.answerNeed(true);
    expect((await allowed)?.granted.messages).toBe(true);
    expect(calls).toContainEqual(["core_plugin_grant", { plugin: view.id, granted: expect.objectContaining({ messages: true }) }]);

    const refused = window.allowedToRead(view);
    window.answerNeed(false);
    expect(await refused).toBeUndefined();
  });

  it("shows the thread over a game on a tap, reading what came, and the newest message when the game closes", async () => {
    const { window, scrollToEnd } = host();
    window.openPlugin(GAME);
    await flushPromises();
    expect(window.playing.value).toBe(true);
    chat("c1")!.messages.push({ id: "in-1", mine: false, text: "nice move!", time: "10:00", sentAt: Date.now() });
    expect(window.lastFromThem.value?.text).toBe("nice move!");
    calls.length = 0;
    await window.peek();
    expect(window.peeking.value).toBe(true);
    expect(calls).toContainEqual(["core_mark_read", { contact: "c1" }]);
    expect(window.lastFromThem.value).toBeUndefined();

    scrollToEnd.mockClear();
    window.closePlugin();
    await flushPromises();
    expect(window.playing.value).toBe(false);
    expect(window.peeking.value).toBe(false);
    expect(scrollToEnd).toHaveBeenCalled();
  });

  it("says goodbye to the plugin when a split view shows another conversation", async () => {
    const { window, chatId, sheet } = host({ split: true });
    window.openPlugin(GAME);
    chatId.value = "c2";
    await flushPromises();
    expect(sheet.close).toHaveBeenCalledTimes(1);

    const page = host();
    page.window.openPlugin(GAME);
    page.chatId.value = "c2";
    await flushPromises();
    expect(page.sheet.close).not.toHaveBeenCalled();
  });
});
