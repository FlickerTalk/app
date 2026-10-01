/**
 * Web and mail addresses inside a message (Ioan, 2026-09-22). The app marks them so they can be
 * opened; it never goes and fetches them, which would tell a stranger's server that the message
 * arrived (Plan §70, §99). Only http(s) and mail are ever links: anything else stays text.
 *
 * 2026-10-02: a place, as a `geo:` URI (RFC 5870) the location plugin writes, is a third kind of
 * piece. The app draws it as a card that opens the phone's own maps app on a tap; no map is ever
 * fetched to show it, which would tell a tile server where the people are.
 */
export interface Piece {
  kind: "text" | "link" | "place";
  text: string;
  href?: string;
  place?: Place;
}

/** A place on Earth, from a `geo:` URI (2026-10-02): degrees, and metres of accuracy if said. */
export interface Place {
  lat: number;
  lon: number;
  accuracy: number | null;
}

const PATTERN = /(https?:\/\/[^\s<>"']+)|([\w.+-]+@[\w-]+\.[\w.-]+)|([gG][eE][oO]:[^\s<>"']+)/g;
/** Trailing punctuation belongs to the sentence, not to the address. */
const TRAILING = /[.,;:!?)\]}'"]+$/;

/** `geo:lat,lon[,alt][;param=value]…`, in WGS 84 (the only system the RFC requires). */
const GEO = /^geo:(-?\d{1,3}(?:\.\d+)?),(-?\d{1,3}(?:\.\d+)?)(?:,-?\d+(?:\.\d+)?)?((?:;[a-z0-9-]+=[^;\s]+)*)$/i;

/** The place a `geo:` URI names, or nothing if it is not a well-formed place on Earth. */
export function placeOf(uri: string): Place | null {
  const found = GEO.exec(uri);
  if (!found) return null;
  const lat = Number(found[1]);
  const lon = Number(found[2]);
  if (Math.abs(lat) > 90 || Math.abs(lon) > 180) return null;
  let accuracy: number | null = null;
  for (const param of found[3].split(";").filter(Boolean)) {
    const [name, value] = param.split("=");
    const key = name.toLowerCase();
    if (key === "crs" && value.toLowerCase() !== "wgs84") return null;
    if (key === "u") {
      if (!/^\d+(?:\.\d+)?$/.test(value)) return null;
      accuracy = Number(value);
    }
  }
  return { lat, lon, accuracy };
}

export function piecesOf(text: string): Piece[] {
  const pieces: Piece[] = [];
  let last = 0;
  let match: RegExpExecArray | null;

  PATTERN.lastIndex = 0;
  while ((match = PATTERN.exec(text)) !== null) {
    const found = match[0].replace(TRAILING, "");
    if (!found) continue;
    let piece: Piece;
    if (match[3] !== undefined) {
      // A malformed place is no place: it stays inside the text around it.
      const place = placeOf(found);
      if (!place) continue;
      piece = { kind: "place", text: found, place };
    } else if (match[1] !== undefined) {
      piece = { kind: "link", text: found, href: found };
    } else {
      piece = { kind: "link", text: found, href: `mailto:${found}` };
    }
    if (match.index > last) pieces.push({ kind: "text", text: text.slice(last, match.index) });
    pieces.push(piece);
    last = match.index + found.length;
  }

  if (last < text.length) pieces.push({ kind: "text", text: text.slice(last) });
  return pieces.length ? pieces : [{ kind: "text", text }];
}

/**
 * The link that opens the phone's own maps app at a place, only when the user taps it: on
 * Android a `geo:` intent (any maps app the user has), elsewhere Apple Maps' link, which the
 * iPhone opens in Maps.
 */
export function mapsLink(place: Place, userAgent: string): string {
  const at = `${place.lat},${place.lon}`;
  return /Android/i.test(userAgent) ? `geo:${at}?q=${at}` : `https://maps.apple.com/?ll=${at}&q=${at}`;
}

/** A message as the chat list shows it: a place in words (`📍 Location`), not its URI. */
export function previewOf(text: string, place: string): string {
  return piecesOf(text)
    .map((piece) => (piece.kind === "place" ? `📍 ${place}` : piece.text))
    .join("");
}
