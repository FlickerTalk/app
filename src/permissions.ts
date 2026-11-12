// installing grants nothing, every permission is its own switch
import {
  alarmOutline,
  archiveOutline,
  chatbubbleEllipsesOutline,
  cloudOutline,
  createOutline,
  globeOutline,
  languageOutline,
  locationOutline,
  swapHorizontalOutline,
} from "ionicons/icons";
import type { PluginPermissions, PluginView } from "./core";
import { isGame } from "./games";
import { t } from "./i18n";

export interface PermissionLine {
  key: string;
  label: string;
  icon: string;
  on: boolean;
}

export function permissionsOf(plugin: PluginView): PermissionLine[] {
  const lines: PermissionLine[] = [];
  if (plugin.asks.messages) {
    lines.push({
      key: "messages",
      label: t("plugins.readsGiven"),
      icon: chatbubbleEllipsesOutline,
      on: plugin.granted.messages,
    });
  }
  for (const host of plugin.asks.network) {
    lines.push({
      key: `network:${host}`,
      label: host,
      icon: globeOutline,
      on: plugin.granted.network.includes(host),
    });
  }
  if (plugin.asks.send !== "nothing") {
    lines.push({
      key: "send",
      label: plugin.asks.send === "auto" ? t("plugins.sendsAlone") : t("plugins.writes"),
      icon: createOutline,
      on: plugin.granted.send !== "nothing",
    });
  }
  const live = isGame(plugin) ? t("games.live") : t("plugins.live");
  if (plugin.asks.live) lines.push({ key: "live", label: live, icon: swapHorizontalOutline, on: !!plugin.granted.live });
  if (plugin.asks.remind) lines.push({ key: "remind", label: t("plugins.remind"), icon: alarmOutline, on: !!plugin.granted.remind });
  if (plugin.asks.drive) lines.push({ key: "drive", label: t("plugins.drive"), icon: cloudOutline, on: !!plugin.granted.drive });
  // the core refuses a game that asks for location
  if (plugin.asks.location && !isGame(plugin)) lines.push({ key: "location", label: t("plugins.location"), icon: locationOutline, on: !!plugin.granted.location });
  if (plugin.asks.translate && !isGame(plugin)) lines.push({ key: "translate", label: t("plugins.translate"), icon: languageOutline, on: !!plugin.granted.translate });
  if (plugin.asks.storage === "large") {
    lines.push({ key: "storage", label: t("plugins.storageLarge"), icon: archiveOutline, on: plugin.granted.storage === "large" });
  }
  return lines;
}

export function withPermission(plugin: PluginView, key: string, on: boolean): PluginPermissions {
  const granted: PluginPermissions = {
    network: [...plugin.granted.network],
    messages: plugin.granted.messages,
    send: plugin.granted.send,
    print: plugin.granted.print,
    live: plugin.granted.live,
    remind: plugin.granted.remind,
    drive: plugin.granted.drive,
    storage: plugin.granted.storage,
    location: plugin.granted.location,
    translate: plugin.granted.translate,
  };
  if (key === "messages") granted.messages = on;
  else if (key === "send") granted.send = on ? plugin.asks.send : "nothing";
  else if (key === "live") granted.live = on;
  else if (key === "remind") granted.remind = on;
  else if (key === "drive") granted.drive = on;
  else if (key === "location") granted.location = on;
  else if (key === "translate") granted.translate = on;
  else if (key === "storage") granted.storage = on ? "large" : "small";
  else {
    const host = key.slice("network:".length);
    granted.network = on ? [...new Set([...granted.network, host])] : granted.network.filter((one) => one !== host);
  }
  return granted;
}

export function missingPermission(plugin: PluginView, key: string): PermissionLine | undefined {
  const line = permissionsOf(plugin).find((one) => one.key === key);
  return line && !line.on ? line : undefined;
}

// must match host_of in ft-core web.rs
export function hostOf(url: string): string | undefined {
  if (!url.startsWith("https://")) return undefined;
  const authority = url.slice("https://".length).split(/[/?#]/)[0];
  if (!authority || authority.includes("@")) return undefined;
  return authority.split(":")[0].toLowerCase() || undefined;
}

export interface PermissionNeed extends PermissionLine {
  answer: (allowed: boolean) => void;
}
