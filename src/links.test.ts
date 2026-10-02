import { describe, expect, it } from "vitest";
import { mapsLink, piecesOf, previewOf } from "./links";
import capabilities from "../src-tauri/capabilities/default.json";

describe("links in a message", () => {
  it("finds a web address and leaves the rest as text", () => {
    expect(piecesOf("mira https://flickertalk.com/add y dime")).toEqual([
      { kind: "text", text: "mira " },
      { kind: "link", text: "https://flickertalk.com/add", href: "https://flickertalk.com/add" },
      { kind: "text", text: " y dime" },
    ]);
  });

  it("finds an email address", () => {
    expect(piecesOf("escribe a info@flickertalk.com")).toEqual([
      { kind: "text", text: "escribe a " },
      { kind: "link", text: "info@flickertalk.com", href: "mailto:info@flickertalk.com" },
    ]);
  });

  it("leaves the punctuation out of the link", () => {
    expect(piecesOf("aquí: https://flickertalk.com/faq.")).toEqual([
      { kind: "text", text: "aquí: " },
      { kind: "link", text: "https://flickertalk.com/faq", href: "https://flickertalk.com/faq" },
      { kind: "text", text: "." },
    ]);
  });

  // Anything that is not a web or mail address stays as the text it was.
  it("never makes a link of something that could run", () => {
    expect(piecesOf("javascript:steal()")).toEqual([{ kind: "text", text: "javascript:steal()" }]);
    expect(piecesOf("file:///etc/passwd")).toEqual([{ kind: "text", text: "file:///etc/passwd" }]);
    expect(piecesOf("data:text/html,<script>")).toEqual([{ kind: "text", text: "data:text/html,<script>" }]);
  });

  it("is one piece of text when there is nothing to link", () => {
    expect(piecesOf("hola, qué tal")).toEqual([{ kind: "text", text: "hola, qué tal" }]);
  });
});

// 2026-10-02: a place, as the location plugin writes it (RFC 5870). The app shows it as a card
// that opens the phone's own maps; it never fetches a map, which would tell a tile server where
// the people are.
describe("places in a message", () => {
  it("finds a geo URI with its accuracy", () => {
    expect(piecesOf("geo:40.41680,-3.70380;u=35")).toEqual([
      { kind: "place", text: "geo:40.41680,-3.70380;u=35", place: { lat: 40.4168, lon: -3.7038, accuracy: 35 } },
    ]);
  });

  it("finds a place inside a sentence, without the full stop", () => {
    expect(piecesOf("here: geo:-33.86882,151.20929;u=12.")).toEqual([
      { kind: "text", text: "here: " },
      { kind: "place", text: "geo:-33.86882,151.20929;u=12", place: { lat: -33.86882, lon: 151.20929, accuracy: 12 } },
      { kind: "text", text: "." },
    ]);
  });

  it("takes a place with no accuracy, with an altitude or in WGS 84 said out loud", () => {
    expect(piecesOf("geo:48.2010,16.3695")[0]).toMatchObject({ kind: "place", place: { lat: 48.201, lon: 16.3695, accuracy: null } });
    expect(piecesOf("geo:48.2010,16.3695,183;u=40")[0]).toMatchObject({ kind: "place", place: { lat: 48.201, lon: 16.3695, accuracy: 40 } });
    expect(piecesOf("GEO:48.2010,16.3695;crs=wgs84;u=40")[0]).toMatchObject({ kind: "place", place: { accuracy: 40 } });
  });

  // A malformed geo URI is not a place: it stays the text it was.
  it("leaves what is not a place as text", () => {
    for (const nowhere of [
      "geo:91.0,10.0",
      "geo:10.0,181.0",
      "geo:abc,def",
      "geo:40.4168",
      "geo:40.4168,-3.7038;u=-5",
      "geo:40.4168,-3.7038;u=far",
      "geo:40.4168,-3.7038;crs=moon-2011;u=3",
      "geo:,",
    ]) {
      expect(piecesOf(nowhere), nowhere).toEqual([{ kind: "text", text: nowhere }]);
    }
  });

  // The tap opens the phone's maps app; nothing is fetched before it. Android takes a geo intent,
  // the iPhone (and anything else) Apple Maps' link.
  it("opens the phone's own maps app", () => {
    const place = { lat: 40.4168, lon: -3.7038, accuracy: 35 };
    expect(mapsLink(place, "Mozilla/5.0 (Linux; Android 16; Lenovo) AppleWebKit/537.36")).toBe("geo:40.4168,-3.7038?q=40.4168,-3.7038");
    expect(mapsLink(place, "Mozilla/5.0 (iPhone; CPU iPhone OS 26_0 like Mac OS X) AppleWebKit/605.1.15")).toBe(
      "https://maps.apple.com/?ll=40.4168,-3.7038&q=40.4168,-3.7038",
    );
  });

  // Tauri's opener only opens what the app's capability lets it: the geo intent and Apple Maps.
  it("may open what the maps link makes", () => {
    const opener = capabilities.permissions.find(
      (one): one is { identifier: string; allow: { url: string }[] } => typeof one === "object" && one.identifier === "opener:allow-open-url",
    );
    const allowed = opener?.allow.map((one) => one.url) ?? [];
    expect(allowed).toContain("geo:*");
    expect(allowed).toContain("https://*");
  });

  // The chat list says "📍 Location" instead of the raw URI.
  it("says a place in a preview in words", () => {
    expect(previewOf("geo:40.41680,-3.70380;u=35", "Location")).toBe("📍 Location");
    expect(previewOf("meet here geo:40.41680,-3.70380;u=35", "Location")).toBe("meet here 📍 Location");
    expect(previewOf("geo:91,0", "Location")).toBe("geo:91,0");
    expect(previewOf("hello", "Location")).toBe("hello");
  });
});
