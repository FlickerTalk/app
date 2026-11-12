// links and places are never fetched, that would tell a stranger's server or a tile server about the message
export interface Piece {
  kind: "text" | "link" | "place";
  text: string;
  href?: string;
  place?: Place;
}

export interface Place {
  lat: number;
  lon: number;
  accuracy: number | null;
}

const PATTERN = /(https?:\/\/[^\s<>"']+)|([\w.+-]+@[\w-]+\.[\w.-]+)|([gG][eE][oO]:[^\s<>"']+)/g;
const TRAILING = /[.,;:!?)\]}'"]+$/;

const GEO = /^geo:(-?\d{1,3}(?:\.\d+)?),(-?\d{1,3}(?:\.\d+)?)(?:,-?\d+(?:\.\d+)?)?((?:;[a-z0-9-]+=[^;\s]+)*)$/i;

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

export function mapsLink(place: Place, userAgent: string): string {
  const at = `${place.lat},${place.lon}`;
  return /Android/i.test(userAgent) ? `geo:${at}?q=${at}` : `https://maps.apple.com/?ll=${at}&q=${at}`;
}

export function previewOf(text: string, place: string): string {
  return piecesOf(text)
    .map((piece) => (piece.kind === "place" ? `📍 ${place}` : piece.text))
    .join("");
}
