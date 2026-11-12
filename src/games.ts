// invites are plain links so an older app still shows them

import type { PluginPermissions, PluginView } from "./core";

const PREFIX = "com.flickertalk.game.";
const HOME = "https://flickertalk.com/games/";
// as is_id in ft-plugins
const NAME = /^[a-z0-9_-]{1,100}$/;
// the exact link only, our host, nothing glued after it
const LINK = /(?:^|\s)https:\/\/flickertalk\.com\/games\/([a-z0-9_-]{1,100})(?=[.,;:!?]*(?:\s|$))/;

export function gameUrl(id: string): string | undefined {
  if (!id.startsWith(PREFIX)) return undefined;
  const name = id.slice(PREFIX.length);
  return NAME.test(name) ? HOME + name : undefined;
}

export function gameIdFromText(text: string): string | undefined {
  const found = LINK.exec(text);
  return found ? PREFIX + found[1] : undefined;
}

// unknown kind is a tool, as the core defaults it
export function isGame(plugin: { kind?: "tool" | "game" }): boolean {
  return plugin.kind === "game";
}

export function needsGameGrant(plugin: PluginView): boolean {
  const live = Boolean(plugin.asks.live) && !plugin.granted.live;
  const send = plugin.asks.send !== "nothing" && plugin.granted.send === "nothing";
  return live || send;
}

export function gameGrant(plugin: PluginView): PluginPermissions {
  // never location or translate, whatever the grant said
  const { location: _nowhere, translate: _untranslated, ...granted } = plugin.granted;
  return {
    ...granted,
    live: Boolean(plugin.asks.live),
    send: plugin.asks.send === "nothing" ? "nothing" : "propose",
  };
}
