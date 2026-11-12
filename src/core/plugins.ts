import { invoke } from "@tauri-apps/api/core";

export type Sending = "nothing" | "propose" | "auto";

export interface PluginPermissions {
  // empty means no network
  network: string[];
  messages: boolean;
  send: Sending;
  print?: boolean;
  live?: boolean;
  remind?: boolean;
  drive?: boolean;
  storage?: "small" | "large";
  location?: boolean;
  translate?: boolean;
}

export interface PluginView {
  id: string;
  name: string;
  version: string;
  asks: PluginPermissions;
  granted: PluginPermissions;
  installedAt: number;
  /** "*\/*" = any */
  opens?: string[];
  views?: string[];
  kind?: PluginKind;
  locales?: PluginLocales;
  icon?: string;
  image?: string;
  hooks?: PluginHooks;
}

export type PluginHookName = "message" | "composer" | "settings";

export interface PluginHookDeclaration {
  label: string;
  icon: string;
  fields?: PluginSettingsField[];
  note?: string;
}

export interface PluginSettingsField {
  key: string;
  type: "select" | "toggle";
  label: string;
}

export type PluginHooks = Partial<Record<PluginHookName, PluginHookDeclaration>>;

export interface PluginSettingsLabels {
  label?: string;
  fields?: Partial<Record<string, string>>;
  note?: string;
}

export type PluginLocales = Record<
  string,
  { name?: string; summary?: string; hooks?: Partial<Record<string, string | PluginSettingsLabels>> }
>;

export type PluginKind = "tool" | "game";

export async function plugins(): Promise<PluginView[]> {
  return invoke<PluginView[]>("core_plugins");
}

export interface OfferedPlugin {
  id: string;
  name: string;
  version: string;
  summary: string;
  size: number;
  installed: boolean;
  carried: boolean;
  kind?: PluginKind;
  locales?: PluginLocales;
  icon?: string;
  image?: string;
}

// the core never updates a plugin while a frame is open
export async function pluginOpen(plugin: string, open: boolean): Promise<void> {
  await invoke("core_plugin_open", { plugin, open });
}

export async function offeredPlugins(): Promise<OfferedPlugin[]> {
  return invoke<OfferedPlugin[]>("core_catalogue");
}

export async function installPlugin(plugin: string): Promise<void> {
  await invoke("core_plugin_add", { plugin });
}

export interface ChatHookState {
  plugin: string;
  autoMessage: boolean;
  autoComposer: boolean;
  lang?: string;
}

export async function chatHooks(conversation: string): Promise<ChatHookState[]> {
  try {
    return (await invoke<ChatHookState[] | undefined>("core_chat_hooks", { conversation })) ?? [];
  } catch {
    return [];
  }
}

export async function setChatHooks(
  conversation: string,
  plugin: string,
  said: { message: boolean; composer: boolean; lang?: string | null },
): Promise<void> {
  await invoke("core_chat_hooks_set", { conversation, plugin, autoMessage: said.message, autoComposer: said.composer, lang: said.lang || null });
}

export async function hookSample(plugin: string, conversation: string): Promise<string> {
  try {
    return (await invoke<string | undefined>("core_hook_sample", { plugin, conversation })) ?? "";
  } catch {
    return "";
  }
}

export async function grantPlugin(plugin: string, granted: PluginPermissions): Promise<void> {
  await invoke("core_plugin_grant", { plugin, granted });
}

export async function removePlugin(plugin: string): Promise<void> {
  await invoke("core_plugin_remove", { plugin });
}
