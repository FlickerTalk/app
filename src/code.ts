/**
 * Code in a message (Ioan, 2026-09-22). A message fenced with ``` is shown as a block of code, in
 * both phones, by the app itself: it is how the text is drawn, not something a plugin should do.
 * The text that travels is the message as it was written, fences included.
 */
const LANGUAGES = [
  "bash",
  "c",
  "cpp",
  "css",
  "go",
  "html",
  "java",
  "javascript",
  "js",
  "json",
  "kotlin",
  "php",
  "python",
  "ruby",
  "rust",
  "sh",
  "sql",
  "swift",
  "ts",
  "typescript",
  "xml",
  "yaml",
];

export interface Code {
  /** The language, only if we know it; never shown as anything but text. */
  language: string;
  code: string;
}

/** The code of a fenced message, or nothing if the message is not one. */
export function readCode(text: string): Code | null {
  const fenced = /^```([^\n]*)\n([\s\S]*?)\n?```$/.exec(text.trim());
  if (!fenced) return null;
  const language = fenced[1].trim().toLowerCase();
  return { language: LANGUAGES.includes(language) ? language : "", code: fenced[2] };
}
