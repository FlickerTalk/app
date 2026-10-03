import { describe, expect, it } from "vitest";
import { gameGrant, gameIdFromText, gameUrl, isGame, needsGameGrant } from "./games";
import type { PluginView } from "./core";

const CHESS = "com.flickertalk.game.chess";
const CHESS_ID = CHESS;

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

  // 2026-10-02 (decision 6 of the plan of the catalogue's translations): the game's name, in the
  // sender's language, stands before the question, never inside it: names are translated now, and
  // "¿Jugamos a Ajedrez?" or a declined name in Russian or German reads wrong. The link closes the
  // text in every language, so this app, and any that reads the link, finds the game.
  it("invites with the game's name before the question and the link at the end, in every language", async () => {
    const catalogues = import.meta.glob<{ games: { inviteText: string } }>("./i18n/*.json", { eager: true, import: "default" });
    expect(Object.keys(catalogues)).toHaveLength(21);
    for (const [path, messages] of Object.entries(catalogues)) {
      const text = messages.games.inviteText;
      expect(text, path).toMatch(/^🎮 \{game\} · [^{}]+ \{url\}$/);
      const sent = text.replace("{game}", "Ajedrez").replace("{url}", "https://flickertalk.com/games/chess");
      expect(gameIdFromText(sent), path).toBe(CHESS);
    }
    // What an app before this wording sent is still an invitation.
    expect(gameIdFromText("🎮 ¿Jugamos a Ajedrez? https://flickertalk.com/games/chess")).toBe(CHESS);
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

  // Plan decision 11: installing grants nothing (§53); the first time a game is played one sheet
  // grants the two things a game asks for, talking to its twin and leaving the result in the chat.
  describe("what a game is granted to be played", () => {
    const CHESS: PluginView = {
      id: CHESS_ID,
      name: "Chess",
      version: "1.0.0",
      kind: "game",
      asks: { network: [], messages: false, send: "propose", live: true },
      granted: { network: [], messages: false, send: "nothing", live: false },
      installedAt: 1,
    };

    it("needs the grant until both are on", () => {
      expect(needsGameGrant(CHESS)).toBe(true);
      expect(needsGameGrant({ ...CHESS, granted: { ...CHESS.granted, live: true } })).toBe(true);
      expect(needsGameGrant({ ...CHESS, granted: { ...CHESS.granted, send: "propose" } })).toBe(true);
      expect(needsGameGrant({ ...CHESS, granted: { ...CHESS.granted, live: true, send: "propose" } })).toBe(false);
      // What it never asked for is never missing.
      expect(needsGameGrant({ ...CHESS, asks: { network: [], messages: false, send: "nothing" } })).toBe(false);
    });

    it("grants the live channel and proposing in the chat together, never more than asked", () => {
      expect(gameGrant(CHESS)).toEqual({ ...CHESS.granted, live: true, send: "propose" });
      // Asked to send by itself, a game still only proposes: the user sends.
      expect(gameGrant({ ...CHESS, asks: { ...CHESS.asks, send: "auto" } }).send).toBe("propose");
      expect(gameGrant({ ...CHESS, asks: { network: [], messages: false, send: "nothing" } })).toEqual({ ...CHESS.granted, live: false, send: "nothing" });
    });

    // A game never learns where the phone is (app#32): the core refuses a game that asks, and one
    // allow never carries it, whatever the grant said before.
    it("never grants a game the phone's position, nor waits for it", () => {
      const asking = { ...CHESS, asks: { ...CHESS.asks, location: true }, granted: { ...CHESS.granted, location: true } };
      expect("location" in gameGrant(asking)).toBe(false);
      expect(needsGameGrant({ ...asking, granted: { ...asking.granted, live: true, send: "propose", location: false } })).toBe(false);
    });
  });
});
