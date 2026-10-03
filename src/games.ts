/**
 * Games (plan 10, app 1.3.0): a game is a plugin the core marks `kind: "game"`, one package per
 * game in the signed catalogue, with the id `com.flickertalk.game.<name>`. Inviting someone is a
 * plain text with the game's page on our site, `https://flickertalk.com/games/<name>`: an older app
 * shows the link, this one turns it into a way to play (plan 10.6). The app carries the three games
 * (2026-10-03), so every phone has them, an iPhone included.
 */

import type { PluginPermissions, PluginView } from "./core";

const PREFIX = "com.flickertalk.game.";
const HOME = "https://flickertalk.com/games/";
/** What a plugin id allows in one of its parts (`is_id` in `ft-plugins`): no dots, no capitals. */
const NAME = /^[a-z0-9_-]{1,100}$/;
/**
 * The exact link, and nothing that looks like it: at the start of the text or after a space, our
 * host only, no port, user, query, fragment or more path, and nothing glued after it except the
 * end of a sentence.
 */
const LINK = /(?:^|\s)https:\/\/flickertalk\.com\/games\/([a-z0-9_-]{1,100})(?=[.,;:!?]*(?:\s|$))/;

/** The page of one of our games, or undefined for anything else. */
export function gameUrl(id: string): string | undefined {
  if (!id.startsWith(PREFIX)) return undefined;
  const name = id.slice(PREFIX.length);
  return NAME.test(name) ? HOME + name : undefined;
}

/** The game a message invites to, by the exact link of its page; undefined if there is none. */
export function gameIdFromText(text: string): string | undefined {
  const found = LINK.exec(text);
  return found ? PREFIX + found[1] : undefined;
}

/** Whether a plugin is a game. A missing or unknown kind is a tool, as the core defaults it. */
export function isGame(plugin: { kind?: "tool" | "game" }): boolean {
  return plugin.kind === "game";
}

/**
 * Whether a game still lacks what it needs to be played (plan decision 11): talking to its twin
 * on the other phone and leaving the result in the chat. Only what it asked for counts.
 */
export function needsGameGrant(plugin: PluginView): boolean {
  const live = Boolean(plugin.asks.live) && !plugin.granted.live;
  const send = plugin.asks.send !== "nothing" && plugin.granted.send === "nothing";
  return live || send;
}

/**
 * What one "allow" grants a game: the live channel and proposing in the chat, together, and
 * never more than it asked for. A game never sends by itself: it proposes, the user sends (§53).
 */
export function gameGrant(plugin: PluginView): PluginPermissions {
  // Never where the phone is (app#32): whatever the grant said, it is not carried over (the core
  // refuses a game that asks for it anyway).
  const { location: _never, ...granted } = plugin.granted;
  return {
    ...granted,
    live: Boolean(plugin.asks.live),
    send: plugin.asks.send === "nothing" ? "nothing" : "propose",
  };
}
