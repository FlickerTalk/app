// Initials for the avatar. A name is split into words and each word gives its first user-perceived
// character: a grapheme cluster when the platform has `Intl.Segmenter` (flags, joined emoji), whole
// code points otherwise. Never `word[0]`, which can be half of a surrogate pair (app#71).

const hasSegmenter = typeof Intl !== "undefined" && typeof Intl.Segmenter === "function";
let segmenter: Intl.Segmenter | undefined;

export function firstCharacter(word: string, segment = hasSegmenter): string {
  if (segment) {
    segmenter ??= new Intl.Segmenter(undefined, { granularity: "grapheme" });
    const first = segmenter.segment(word)[Symbol.iterator]().next();
    return first.done ? "" : first.value.segment;
  }
  return [...word][0] ?? "";
}

export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((word) => firstCharacter(word).toUpperCase())
    .join("");
}
