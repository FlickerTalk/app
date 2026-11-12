// never word[0], it can be half a surrogate pair

const hasSegmenter = typeof Intl !== "undefined" && typeof Intl.Segmenter === "function";
let segmenter: Intl.Segmenter | undefined;

// flags are pairs of regional indicators, not pictographs
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
