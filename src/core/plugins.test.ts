import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import {
  chatHooks,
  grantPlugin,
  hookSample,
  installPlugin,
  offeredPlugins,
  pluginOpen,
  plugins,
  removePlugin,
  setChatHooks,
  type PluginPermissions,
} from "./plugins";

describe("plugins", () => {
  beforeEach(() => {
    tauri.invoke.mockReset();
    tauri.invoke.mockResolvedValue(undefined);
  });

  it("lists what is installed and what the catalogue offers", async () => {
    tauri.invoke.mockResolvedValueOnce([{ id: "com.flickertalk.notes" }]);
    tauri.invoke.mockResolvedValueOnce([{ id: "com.flickertalk.board" }]);
    expect(await plugins()).toEqual([{ id: "com.flickertalk.notes" }]);
    expect(await offeredPlugins()).toEqual([{ id: "com.flickertalk.board" }]);
    expect(tauri.invoke.mock.calls).toEqual([["core_plugins"], ["core_catalogue"]]);
  });

  it("installs, grants, opens, closes and removes a plugin through the core", async () => {
    const granted: PluginPermissions = { network: ["api.example.com"], messages: true, send: "propose" };
    await installPlugin("com.flickertalk.notes");
    await grantPlugin("com.flickertalk.notes", granted);
    await pluginOpen("com.flickertalk.notes", true);
    await pluginOpen("com.flickertalk.notes", false);
    await removePlugin("com.flickertalk.notes");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_plugin_add", { plugin: "com.flickertalk.notes" }],
      ["core_plugin_grant", { plugin: "com.flickertalk.notes", granted }],
      ["core_plugin_open", { plugin: "com.flickertalk.notes", open: true }],
      ["core_plugin_open", { plugin: "com.flickertalk.notes", open: false }],
      ["core_plugin_remove", { plugin: "com.flickertalk.notes" }],
    ]);
  });

  it("runs no tool by itself in a conversation when the core cannot say", async () => {
    tauri.invoke.mockResolvedValueOnce(undefined);
    expect(await chatHooks("ft_bob")).toEqual([]);
    tauri.invoke.mockRejectedValueOnce(new Error("no core"));
    expect(await chatHooks("ft_bob")).toEqual([]);
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_chat_hooks", { conversation: "ft_bob" }],
      ["core_chat_hooks", { conversation: "ft_bob" }],
    ]);
  });

  it("saves what a tool does by itself in a conversation, with no language when none is chosen", async () => {
    await setChatHooks("ft_bob", "com.flickertalk.translate", { message: true, composer: false });
    await setChatHooks("ft_bob", "com.flickertalk.translate", { message: false, composer: true, lang: "" });
    await setChatHooks("circle1", "com.flickertalk.translate", { message: true, composer: true, lang: "es" });
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_chat_hooks_set", { conversation: "ft_bob", plugin: "com.flickertalk.translate", autoMessage: true, autoComposer: false, lang: null }],
      ["core_chat_hooks_set", { conversation: "ft_bob", plugin: "com.flickertalk.translate", autoMessage: false, autoComposer: true, lang: null }],
      ["core_chat_hooks_set", { conversation: "circle1", plugin: "com.flickertalk.translate", autoMessage: true, autoComposer: true, lang: "es" }],
    ]);
  });

  it("hands over the settings sample the core has, and nothing when it has none", async () => {
    tauri.invoke.mockResolvedValueOnce("salut");
    expect(await hookSample("com.flickertalk.translate", "ft_bob")).toBe("salut");
    tauri.invoke.mockResolvedValueOnce(undefined);
    expect(await hookSample("com.flickertalk.translate", "ft_bob")).toBe("");
    tauri.invoke.mockRejectedValueOnce(new Error("no core"));
    expect(await hookSample("com.flickertalk.translate", "ft_bob")).toBe("");
    expect(tauri.invoke).toHaveBeenLastCalledWith("core_hook_sample", { plugin: "com.flickertalk.translate", conversation: "ft_bob" });
  });
});
