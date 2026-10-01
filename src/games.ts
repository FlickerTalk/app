/**
 * Games (plan 10, app 1.3.0): a game is a plugin the core marks `kind: "game"`, one package per
 * game in the signed catalogue, with the id `com.flickertalk.game.<name>`. Inviting someone is a
 * plain text with the game's page on our site, `https://flickertalk.com/games/<name>`: an older app
 * or an iPhone shows the link, this one turns it into a way to play (plan 10.6).
 */

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
 * Whether this phone can have games: only where plugins are downloaded, which is not iOS (App
 * Store 4.7, §52; `downloads()` in the core). The core does not tell the WebView, so it is read
 * from the WebView itself: an iPhone says so, an iPad says it is a Mac but has a touch screen.
 */
export function gamesAvailable(agent = navigator.userAgent, touch = navigator.maxTouchPoints ?? 0): boolean {
  if (/iPhone|iPad|iPod/.test(agent)) return false;
  return !(/Macintosh/.test(agent) && touch > 1);
}
