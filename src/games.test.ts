import { describe, expect, it } from "vitest";
import { gameIdFromText, gameUrl, gamesAvailable, isGame } from "./games";

const CHESS = "com.flickertalk.game.chess";

describe("games", () => {
  // Plan 10.6: the invitation is plain text with a link to the game's page on our site.
  it("links a game of ours to its page on flickertalk.com", () => {
    expect(gameUrl(CHESS)).toBe("https://flickertalk.com/games/chess");
    expect(gameUrl("com.flickertalk.game.connect4")).toBe("https://flickertalk.com/games/connect4");
  });

  it("has no page for what is not one of our games", () => {
    expect(gameUrl("com.flickertalk.markdown")).toBeUndefined();
    expect(gameUrl("com.example.game.chess")).toBeUndefined();
    expect(gameUrl("com.flickertalk.game.")).toBeUndefined();
    expect(gameUrl("com.flickertalk.game.chess.extra")).toBeUndefined();
  });

  it("reads the game of an exact link in a message", () => {
    expect(gameIdFromText("https://flickertalk.com/games/chess")).toBe(CHESS);
    expect(gameIdFromText("🎮 Shall we play Chess? https://flickertalk.com/games/chess")).toBe(CHESS);
    expect(gameIdFromText("https://flickertalk.com/games/connect4 now?")).toBe("com.flickertalk.game.connect4");
    // The end of a sentence is not part of the link.
    expect(gameIdFromText("play https://flickertalk.com/games/chess.")).toBe(CHESS);
    expect(gameIdFromText("line one\nhttps://flickertalk.com/games/tic_tac-toe")).toBe("com.flickertalk.game.tic_tac-toe");
  });

  // Anyone can write that text; only the exact link of our site counts, nothing that looks like it.
  it.each([
    ["another host", "https://flickertalk.org/games/chess"],
    ["a host that only starts like ours", "https://flickertalk.com.evil.com/games/chess"],
    ["a subdomain", "https://www.flickertalk.com/games/chess"],
    ["a host that ends like ours", "https://notflickertalk.com/games/chess"],
    ["plain http", "http://flickertalk.com/games/chess"],
    ["no scheme", "flickertalk.com/games/chess"],
    ["a port", "https://flickertalk.com:443/games/chess"],
    ["a user", "https://me@flickertalk.com/games/chess"],
    ["more path", "https://flickertalk.com/games/chess/extra"],
    ["a trailing slash", "https://flickertalk.com/games/chess/"],
    ["a query", "https://flickertalk.com/games/chess?x=1"],
    ["a fragment", "https://flickertalk.com/games/chess#x"],
    ["another section", "https://flickertalk.com/plugins/chess"],
    ["no game", "https://flickertalk.com/games/"],
    ["an upper-case game", "https://flickertalk.com/games/Chess"],
    ["an upper-case host", "https://FlickerTalk.com/games/chess"],
    ["an upper-case scheme", "HTTPS://flickertalk.com/games/chess"],
    ["a look-alike letter", "https://flickertаlk.com/games/chess"],
    ["a dot in the game", "https://flickertalk.com/games/chess.evil"],
    ["glued to other text", "xhttps://flickertalk.com/games/chess"],
    ["inside another link", "https://evil.com/?https://flickertalk.com/games/chess"],
    ["an invisible character after it", "https://flickertalk.com/games/chess​x"],
  ])("ignores %s", (_case, text) => {
    expect(gameIdFromText(text)).toBeUndefined();
  });

  // A plugin is a tool unless the core says it is a game; an older or unknown kind is a tool.
  it("tells a game from a tool", () => {
    expect(isGame({ kind: "game" })).toBe(true);
    expect(isGame({ kind: "tool" })).toBe(false);
    expect(isGame({})).toBe(false);
    expect(isGame({ kind: "toy" as never })).toBe(false);
  });

  // Plan 10.3: no downloads on iOS, so no games there (App Store 4.7, §52).
  it("offers games where plugins are downloaded, not on an iPhone or an iPad", () => {
    const android = "Mozilla/5.0 (Linux; Android 14; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0 Mobile Safari/537.36";
    const iphone = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";
    const ipad = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)";
    expect(gamesAvailable(android, 5)).toBe(true);
    expect(gamesAvailable(iphone, 5)).toBe(false);
    // An iPad's WebView says it is a Mac; a Mac has no touch screen.
    expect(gamesAvailable(ipad, 5)).toBe(false);
    expect(gamesAvailable(ipad, 0)).toBe(true);
  });
});
