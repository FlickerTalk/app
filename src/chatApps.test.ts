import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { defineComponent, h, ref } from "vue";
import { calls } from "./__tests__/seed";
import { installTauri } from "./__tests__/tauri";
import type { ChatMessage } from "./core";
import { offered, premiumLocked, refreshPlugins } from "./plugins";
import type { OpenPlugin } from "./pluginWindow";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

import { useChatApps } from "./chatApps";

const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
};
const CHESS = {
  id: "com.flickertalk.game.chess",
  name: "Chess",
  version: "1.0.0",
  kind: "game",
  asks: { network: [], messages: false, send: "propose", live: true },
  granted: { network: [], messages: false, send: "propose", live: true },
  installedAt: 2,
};
const UNGRANTED = { ...CHESS, granted: { network: [], messages: false, send: "nothing", live: false } };
const GO = { id: "com.flickertalk.game.go", name: "Go", version: "1.0.0", summary: "Play go.", size: 1_200_000, installed: false, carried: false, kind: "game" };

async function core({ installed = [] as object[], catalogue = [] as object[], addFails = false } = {}) {
  let list = installed as Array<Record<string, unknown>>;
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_plugins") return list;
    if (command === "core_catalogue") return catalogue;
    if (command === "core_plugin_add") {
      if (addFails) throw new Error("download failed");
      list = [...list, { ...UNGRANTED, id: GO.id, name: GO.name }];
    }
    if (command === "core_plugin_grant") list = list.map((one) => (one.id === args?.plugin ? { ...one, granted: args?.granted } : one));
    return undefined;
  });
  await refreshPlugins();
  calls.length = 0;
}

function host({ messages = [] as ChatMessage[], play = undefined as string | undefined } = {}) {
  const draft = ref("");
  const error = ref("");
  const plugin = ref<OpenPlugin | null>(null);
  const openPlugin = vi.fn();
  const asked = ref(play);
  let apps!: ReturnType<typeof useChatApps>;
  mount(
    defineComponent({
      setup() {
        apps = useChatApps({ messages: () => messages, play: () => asked.value, draft, error, plugin, openPlugin });
        return () => h("div");
      },
    }),
  );
  return { apps, draft, error, plugin, openPlugin, asked };
}

describe("useChatApps", () => {
  beforeEach(() => {
    push.mockClear();
    offered.value = [];
  });
  afterEach(() => (premiumLocked.value = false));

  it("opens on the tools when there is one, on the games otherwise, and reads the list again", async () => {
    await core({ installed: [CODE, CHESS] });
    const withTool = host();
    withTool.apps.openApps();
    expect(withTool.apps.showApps.value).toBe(true);
    expect(withTool.apps.appsTab.value).toBe("tools");
    await flushPromises();
    expect(calls.map(([command]) => command)).toEqual(expect.arrayContaining(["core_plugins", "core_plan"]));

    await core({ installed: [CHESS] });
    const gamesOnly = host();
    gamesOnly.apps.openApps();
    expect(gamesOnly.apps.appsTab.value).toBe("games");
  });

  it("opens a tool in this conversation, or leads a locked one to Premium", async () => {
    await core({ installed: [CODE] });
    const { apps, openPlugin } = host();
    apps.showApps.value = true;
    apps.useApp(CODE.id);
    expect(apps.showApps.value).toBe(false);
    expect(openPlugin).toHaveBeenCalledWith({ id: CODE.id, name: "Code block", sending: "nothing", live: false });

    premiumLocked.value = true;
    apps.useApp(CODE.id);
    expect(push).toHaveBeenCalledWith("/tabs/settings#premium");
    expect(openPlugin).toHaveBeenCalledTimes(1);
  });

  it("plays an allowed game at once, asks first for one that is not, and never opens a tool", async () => {
    await core({ installed: [CODE, CHESS] });
    const allowed = host();
    allowed.apps.playGame(CHESS.id);
    expect(allowed.openPlugin).toHaveBeenCalledWith({ id: CHESS.id, name: "Chess", sending: "propose", live: true, game: true });
    allowed.apps.playGame(CODE.id);
    expect(allowed.openPlugin).toHaveBeenCalledTimes(1);
    expect(allowed.apps.asking.value).toBeNull();

    await core({ installed: [UNGRANTED] });
    const ungranted = host();
    ungranted.apps.playGame(CHESS.id);
    expect(ungranted.openPlugin).not.toHaveBeenCalled();
    expect(ungranted.apps.asking.value).toMatchObject({ id: CHESS.id, name: "Chess" });
    expect(ungranted.apps.asking.value?.size).toBeUndefined();
    await ungranted.apps.allowGame();
    expect(calls).toContainEqual(["core_plugin_grant", { plugin: CHESS.id, granted: { network: [], messages: false, send: "propose", live: true } }]);
    expect(ungranted.openPlugin).toHaveBeenCalledWith(expect.objectContaining({ id: CHESS.id, live: true, game: true }));
  });

  it("installs a game from the catalogue before playing it, and says so when it cannot", async () => {
    await core({ installed: [] });
    offered.value = [GO as never];
    const { apps, openPlugin, error } = host();
    apps.playGame(GO.id);
    expect(apps.asking.value).toMatchObject({ id: GO.id, size: 1_200_000 });
    await apps.allowGame();
    expect(calls).toContainEqual(["core_plugin_add", { plugin: GO.id }]);
    expect(openPlugin).toHaveBeenCalledWith(expect.objectContaining({ id: GO.id, game: true }));

    await core({ installed: [], addFails: true });
    const failing = host();
    failing.apps.playGame(GO.id);
    await failing.apps.allowGame();
    expect(failing.error.value).toBe("The game could not be installed. Check your connection and try again.");
    expect(failing.openPlugin).not.toHaveBeenCalled();
    expect(error.value).toBe("");
  });

  it("leaves an invitation after what is already written", async () => {
    await core({ installed: [CHESS] });
    const { apps, draft, plugin } = host();
    const chess = (await refreshPlugins())[0];
    apps.invite(chess);
    expect(draft.value).toBe("🎮 Chess · Shall we play? https://flickertalk.com/games/chess");
    draft.value = "hey ";
    plugin.value = { ...chess, sending: "propose", live: true, game: true, contact: "c1", opening: 1 };
    apps.inviteToPlaying();
    expect(draft.value).toBe("hey 🎮 Chess · Shall we play? https://flickertalk.com/games/chess");
  });

  it("opens the game the address asks for once the tools are known", async () => {
    await core({ installed: [CHESS] });
    const { apps, openPlugin, asked } = host({ play: CHESS.id });
    expect(openPlugin).not.toHaveBeenCalled();
    apps.loaded();
    expect(openPlugin).toHaveBeenCalledTimes(1);
    asked.value = undefined;
    await flushPromises();
    asked.value = CHESS.id;
    await flushPromises();
    expect(openPlugin).toHaveBeenCalledTimes(2);
  });

  it("reads the catalogue only for an invitation to a game that is not installed", async () => {
    const invite = (id: string, text: string): ChatMessage => ({ id, mine: false, text, time: "09:00", sentAt: 1 });
    await core({ installed: [CHESS], catalogue: [GO] });
    host({ messages: [invite("i1", "🎮 https://flickertalk.com/games/chess")] }).apps.loaded();
    await flushPromises();
    expect(calls.some(([command]) => command === "core_catalogue")).toBe(false);

    host({ messages: [invite("i2", "🎮 https://flickertalk.com/games/go")] }).apps.loaded();
    await flushPromises();
    expect(calls.filter(([command]) => command === "core_catalogue")).toHaveLength(1);
  });
});
