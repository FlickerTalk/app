// the fences travel with the text, only the drawing changes
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
  // shown as text only
  language: string;
  code: string;
}

export function readCode(text: string): Code | null {
  const fenced = /^```([^\n]*)\n([\s\S]*?)\n?```$/.exec(text.trim());
  if (!fenced) return null;
  const language = fenced[1].trim().toLowerCase();
  return { language: LANGUAGES.includes(language) ? language : "", code: fenced[2] };
}
