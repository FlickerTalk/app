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

/** What `followPhoneLanguage` reads the phone's languages from and listens on. */
export type LanguageSource = Pick<Window, "addEventListener"> & { navigator: Pick<Navigator, "languages" | "language"> };

/**
 * The texts follow the phone's language: at start, and in place when it changes while the app
 * runs (app#82: on Android the activity is no longer recreated for it, so neither is the page).
 */
export function followPhoneLanguage(source: LanguageSource = window): Promise<void> {
  const current = () => {
    const { navigator } = source;
    return setLocale(pickLocale(navigator.languages ?? [navigator.language]));
  };
  source.addEventListener("languagechange", () => void current());
  return current();
}

// Where the week starts when the WebView has no week data (CLDR's `weekData`, 2026-10-06): the
// regions that start it on Sunday, Saturday or Friday; every other one starts on Monday.
const SUNDAY = new Set(
  "AG AS BD BR BS BT BW BZ CA CO DM DO ET GT GU HK HN ID IL IN JM JP KE KH KR LA MH MM MO MT MX MZ NI NP PA PE PH PK PR PT PY SA SG SV TH TT TW UM US VE VI WS YE ZA ZW".split(" "),
);
const SATURDAY = new Set("AE AF BH DJ DZ EG IQ IR JO KW LY OM QA SD SY".split(" "));
const FRIDAY = new Set(["MV"]);

type WeekLocale = Intl.Locale & { getWeekInfo?: () => { firstDay: number } | undefined; weekInfo?: { firstDay: number } };

/**
 * The first day of the week for the app's language, as Ionic's picker counts it (0 is Sunday):
 * in the phone's region when the phone speaks that language (es-MX starts on Sunday, es-ES on
 * Monday), else in the language's usual region (QA of 1.4.0, 2026-10-06).
 */
export function firstDayOfWeek(locale: string, phone: readonly string[] = navigator.languages ?? [navigator.language]): number {
  const tag = phone.find((one) => match(one) === locale) ?? locale;
  let place: WeekLocale;
  try {
    place = new Intl.Locale(tag).maximize() as WeekLocale;
  } catch {
    return 1;
  }
  const info = place.getWeekInfo?.() ?? place.weekInfo;
  if (info?.firstDay) return info.firstDay % 7;
  const region = place.region ?? "";
  if (SUNDAY.has(region)) return 0;
  if (SATURDAY.has(region)) return 6;
  if (FRIDAY.has(region)) return 5;
  return 1;
}
