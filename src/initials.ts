// Initials for the avatar. A name is split into words and each word gives its first user-perceived
// character: a grapheme cluster when the platform has `Intl.Segmenter` (flags, joined emoji), whole
// code points otherwise. Never `word[0]`, which can be half of a surrogate pair (app#71).
// Only a letter, a digit or an emoji counts: punctuation and symbols in front of a word are skipped,
// and a word with none of them gives no initial at all, so "Marcos (escalada)" is "ME" (app#158).
// A name with none of them anywhere still gets its first character, so the avatar is never blank.

const hasSegmenter = typeof Intl !== "undefined" && typeof Intl.Segmenter === "function";
let segmenter: Intl.Segmenter | undefined;

// A flag is a pair of regional indicators, which are not pictographs themselves.
const INITIAL = /^[\p{L}\p{N}\p{Extended_Pictographic}\p{Regional_Indicator}]/u;

function* characters(word: string, segment: boolean): Generator<string, void> {
  if (!segment) {
    yield* word;
    return;
  }
  segmenter ??= new Intl.Segmenter(undefined, { granularity: "grapheme" });
  for (const { segment: character } of segmenter.segment(word)) yield character;
}

export function firstCharacter(word: string, segment = hasSegmenter): string {
  for (const character of characters(word, segment)) {
    if (INITIAL.test(character)) return character;
  }
  return "";
}

export function initials(name: string): string {
  const found = name
    .split(/\s+/)
    .map((word) => firstCharacter(word))
    .filter(Boolean)
    .slice(0, 2);
  if (found.length === 0) {
    const first = characters(name.trim(), hasSegmenter).next();
    return first.done ? "" : first.value;
  }
  return found.map((initial) => initial.toUpperCase()).join("");
}
