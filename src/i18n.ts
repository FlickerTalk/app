// English is the source language; every other catalogue translates it key by key and loads only
// when the phone asks for it. The app follows the system language (no picker in Settings).
import { createI18n } from "vue-i18n";
import en from "./i18n/en.json";

export const LOCALES = [
  "en", "es", "pt", "fr", "de", "it", "ro", "ru", "uk", "pl", "tr", "ar",
  "hi", "bn", "id", "vi", "th", "ja", "ko", "zh-CN", "zh-TW",
] as const;
export type Locale = (typeof LOCALES)[number];

const RTL = new Set<string>(["ar"]);

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: "en",
  fallbackLocale: "en",
  messages: { en } as Record<string, typeof en>,
});

export const t = i18n.global.t;

const catalogues = import.meta.glob<typeof en>(["./i18n/*.json", "!./i18n/en.json"], {
  import: "default",
});

export function isRtl(locale: string): boolean {
  return RTL.has(locale);
}

// One BCP 47 tag → one of ours, or undefined. Chinese goes by script (Hant → traditional), and
// Android's legacy "in" still means Indonesian.
function match(tag: string): Locale | undefined {
  const [language, ...rest] = tag.toLowerCase().split(/[-_]/);
  if (language === "zh") {
    return rest.some((part) => ["hant", "tw", "hk", "mo"].includes(part)) ? "zh-TW" : "zh-CN";
  }
  const base = language === "in" ? "id" : language;
  return LOCALES.find((locale) => locale === base);
}

export function pickLocale(preferred: readonly string[]): Locale {
  for (const tag of preferred) {
    const locale = match(tag);
    if (locale) return locale;
  }
  return "en";
}

export async function setLocale(locale: Locale): Promise<void> {
  if (!i18n.global.availableLocales.includes(locale)) {
    const load = catalogues[`./i18n/${locale}.json`];
    if (!load) return;
    i18n.global.setLocaleMessage(locale, await load());
  }
  i18n.global.locale.value = locale;
  document.documentElement.lang = locale;
  document.documentElement.dir = isRtl(locale) ? "rtl" : "ltr";
}
