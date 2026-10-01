/**
 * What a plugin asked for and what it was granted, one switch per permission (§53). Plan §53: a
 * plugin is granted nothing by installing; every permission is shown on its own and can be taken
 * back at any time. Shared by the tools in Settings and the games section.
 */
import {
  alarmOutline,
  archiveOutline,
  chatbubbleEllipsesOutline,
  cloudOutline,
  createOutline,
  globeOutline,
  swapHorizontalOutline,
} from "ionicons/icons";
import type { PluginPermissions, PluginView } from "./core";
import { t } from "./i18n";

export interface PermissionLine {
  key: string;
  label: string;
  icon: string;
  on: boolean;
}

/** What a plugin asks for, one line per permission. */
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
  // 2026-09-27: what the board, the notes and the drive ask for, each on its own switch.
  if (plugin.asks.live) lines.push({ key: "live", label: t("plugins.live"), icon: swapHorizontalOutline, on: !!plugin.granted.live });
  if (plugin.asks.remind) lines.push({ key: "remind", label: t("plugins.remind"), icon: alarmOutline, on: !!plugin.granted.remind });
  if (plugin.asks.drive) lines.push({ key: "drive", label: t("plugins.drive"), icon: cloudOutline, on: !!plugin.granted.drive });
  if (plugin.asks.storage === "large") {
    lines.push({ key: "storage", label: t("plugins.storageLarge"), icon: archiveOutline, on: plugin.granted.storage === "large" });
  }
  return lines;
}

/** What the plugin is granted once the switch `key` is turned on or off; the rest stays. */
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
  };
  if (key === "messages") granted.messages = on;
  else if (key === "send") granted.send = on ? plugin.asks.send : "nothing";
  else if (key === "live") granted.live = on;
  else if (key === "remind") granted.remind = on;
  else if (key === "drive") granted.drive = on;
  else if (key === "storage") granted.storage = on ? "large" : "small";
  else {
    const host = key.slice("network:".length);
    granted.network = on ? [...new Set([...granted.network, host])] : granted.network.filter((one) => one !== host);
  }
  return granted;
}
