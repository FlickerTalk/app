/**
 * Web and mail addresses inside a message (Ioan, 2026-09-22). The app marks them so they can be
 * opened; it never goes and fetches them, which would tell a stranger's server that the message
 * arrived (Plan §70, §99). Only http(s) and mail are ever links: anything else stays text.
 */
export interface Piece {
  kind: "text" | "link";
  text: string;
  href?: string;
}

const PATTERN = /(https?:\/\/[^\s<>"']+)|([\w.+-]+@[\w-]+\.[\w.-]+)/g;
/** Trailing punctuation belongs to the sentence, not to the address. */
const TRAILING = /[.,;:!?)\]}'"]+$/;

export function piecesOf(text: string): Piece[] {
  const pieces: Piece[] = [];
  let last = 0;
  let match: RegExpExecArray | null;

  PATTERN.lastIndex = 0;
  while ((match = PATTERN.exec(text)) !== null) {
    const found = match[0].replace(TRAILING, "");
    if (!found) continue;
    if (match.index > last) pieces.push({ kind: "text", text: text.slice(last, match.index) });
    pieces.push(
      match[1] !== undefined
        ? { kind: "link", text: found, href: found }
        : { kind: "link", text: found, href: `mailto:${found}` },
    );
    last = match.index + found.length;
  }

  if (last < text.length) pieces.push({ kind: "text", text: text.slice(last) });
  return pieces.length ? pieces : [{ kind: "text", text }];
}
