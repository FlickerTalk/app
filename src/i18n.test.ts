import { afterEach, describe, expect, it, vi } from "vitest";
import { firstDayOfWeek, followPhoneLanguage, i18n, isRtl, type Locale, LOCALES, pickLocale, setLocale, t } from "./i18n";
import en from "./i18n/en.json";

function leaves(node: unknown, path: string[] = []): [string, unknown][] {
  if (typeof node !== "object" || node === null) return [[path.join("."), node]];
  return Object.entries(node).flatMap(([key, value]) => leaves(value, [...path, key]));
}

function placeholders(text: string): string[] {
  return [...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
}

const catalogues = import.meta.glob<Record<string, unknown>>("./i18n/*.json", {
  eager: true,
  import: "default",
});
const translations = Object.entries(catalogues)
  .map(([path, messages]) => [path.replace(/^.*\/(.+)\.json$/, "$1"), messages] as const)
  .filter(([locale]) => locale !== "en");

/** A key whose translation may legitimately be the English text, and in which languages. */
interface SameAsEnglish {
  locales: readonly Locale[];
  reason: string;
}

const ALL_TRANSLATED = LOCALES.filter((locale) => locale !== "en");
const LATIN_LOANWORDS = ["de", "es", "fr", "id", "it", "pl", "pt", "ro", "tr", "vi"] as const;
const SAME_WORD = "the same word (or an established loanword) in that language";
const THEME_NAME = "a theme name, kept as a proper name or the same word";

// Every other value must differ from English: an identical one is a text nobody translated.
const SAME_AS_ENGLISH: Record<string, SameAsEnglish> = {
  "app.name": { locales: ALL_TRANSLATED, reason: "the product name is never translated" },
  "session.pin": {
    locales: [
      "ar", "bn", "de", "es", "fr", "hi", "id", "it", "ja",
      "ko", "pl", "pt", "ro", "th", "tr", "vi", "zh-CN", "zh-TW",
    ],
    reason: '"PIN" is used as is',
  },
  "circle.admin": {
    locales: ["de", "es", "fr", "it", "pt", "id", "pl", "ro"],
    reason: '"Admin" is the short badge these languages use as a loanword',
  },
  "chat.accuracy": {
    locales: ["de", "es", "fr", "id", "it", "ja", "pl", "pt", "ro", "tr", "vi"],
    reason: "only a symbol and a unit",
  },
  "contact.hours": {
    locales: ["es", "fr", "it", "pl", "pt", "ro"],
    reason: "unit abbreviation shared with English",
  },
  "contact.minutes": {
    locales: ["es", "fr", "it", "pl", "pt", "ro"],
    reason: "unit abbreviation shared with English",
  },
  "chat.emoji": { locales: LATIN_LOANWORDS, reason: '"Emoji" is the word in that language' },
  "emoji.open": { locales: LATIN_LOANWORDS, reason: '"Emoji" is the word in that language' },
  "contact.reasons.spam": { locales: LATIN_LOANWORDS, reason: '"Spam" is the word in that language' },
  "colors.mono": { locales: LATIN_LOANWORDS, reason: THEME_NAME },
  "colors.aurora": { locales: ["de", "es", "id", "it", "pt", "tr", "vi"], reason: THEME_NAME },
  "colors.lime": { locales: ["id", "it", "ro", "vi"], reason: THEME_NAME },
  "colors.ember": { locales: ["id", "vi"], reason: THEME_NAME },
  "calls.video": { locales: ["de", "es", "id", "it", "ro", "vi"], reason: SAME_WORD },
  "contact.video": { locales: ["de", "id", "it", "ro", "vi"], reason: SAME_WORD },
  "plugins.title": { locales: ["de", "es", "fr", "pt"], reason: SAME_WORD },
  "settings.plugins": { locales: ["de", "es", "fr", "pt"], reason: SAME_WORD },
  "tabs.plugins": { locales: ["de", "es", "fr", "pt"], reason: SAME_WORD },
  "chat.edit": { locales: ["id"], reason: SAME_WORD },
  "settings.backup": { locales: ["de", "it", "pt", "ro"], reason: SAME_WORD },
  "settings.plan": { locales: ["es", "pl", "ro", "tr"], reason: SAME_WORD },
  "chat.direct": { locales: ["fr", "ro"], reason: SAME_WORD },
  "chat.file": { locales: ["id", "it"], reason: SAME_WORD },
  "chat.pause": { locales: ["de", "fr"], reason: SAME_WORD },
  "settings.version": { locales: ["de", "fr"], reason: SAME_WORD },
  "tabs.chats": { locales: ["de", "es"], reason: SAME_WORD },
  "calls.camera": { locales: ["vi"], reason: SAME_WORD },
  "calls.speaker": { locales: ["id"], reason: SAME_WORD },
  "chat.message": { locales: ["fr"], reason: SAME_WORD },
  "contact.acceptsChat": { locales: ["fr"], reason: SAME_WORD },
  "emoji.nature": { locales: ["fr"], reason: SAME_WORD },
  "session.toggle": { locales: ["fr"], reason: SAME_WORD },
  "settings.session": { locales: ["fr"], reason: SAME_WORD },
  "settings.color": { locales: ["es"], reason: SAME_WORD },
  "settings.privacy": { locales: ["it"], reason: SAME_WORD },
  "settings.system": { locales: ["de"], reason: SAME_WORD },
};

function mayEqualEnglish(key: string, locale: string): boolean {
  return SAME_AS_ENGLISH[key]?.locales.some((listed) => listed === locale) ?? false;
}

describe("i18n", () => {
  afterEach(async () => {
    await setLocale("en");
  });

  it("starts in English, the source language", () => {
    expect(i18n.global.locale.value).toBe("en");
    expect(t("tabs.chats")).toBe("Chats");
  });

  it("has no empty entries in the catalogue", () => {
    for (const [key, value] of leaves(en)) {
      expect(typeof value, key).toBe("string");
      expect(String(value).trim(), key).not.toBe("");
    }
  });

  it("ships one catalogue per supported language, and nothing else", () => {
    expect(translations.map(([locale]) => locale).sort()).toEqual(
      LOCALES.filter((locale) => locale !== "en").sort(),
    );
    expect(LOCALES).toEqual(
      expect.arrayContaining([
        "es", "pt", "fr", "de", "it", "ru", "uk", "pl", "tr", "ar",
        "hi", "bn", "id", "ja", "ko", "zh-CN", "zh-TW", "vi", "th", "ro",
      ]),
    );
  });

  describe.each(translations)("the %s catalogue", (locale, messages) => {
    const translated = new Map(leaves(messages));

    it("has exactly the English keys", () => {
      expect([...translated.keys()].sort()).toEqual(leaves(en).map(([key]) => key).sort());
    });

    it("fills every entry and keeps its placeholders", () => {
      for (const [key, source] of leaves(en)) {
        const value = translated.get(key);
        expect(typeof value, key).toBe("string");
        expect(String(value).trim(), key).not.toBe("");
        expect(placeholders(String(value)), key).toEqual(placeholders(String(source)));
        // vue-i18n reads | (plural), @ (linked message) and $ as syntax, not text.
        expect(String(value), key).not.toMatch(/[|@$]/);
      }
    });

    it("keeps the product name untranslated", () => {
      for (const [key, source] of leaves(en)) {
        if (String(source).includes("FlickerTalk")) {
          expect(String(translated.get(key)), key).toContain("FlickerTalk");
        }
      }
    });

    it("translates every entry, apart from the listed ones that stay as in English", () => {
      const untranslated = leaves(en)
        .filter(([key, source]) => translated.get(key) === source && !mayEqualEnglish(key, locale))
        .map(([key]) => key);
      expect(untranslated, `${locale}: entries still in English`).toEqual([]);
    });
  });

  it("lists only entries that exist and really are the same as in English", () => {
    const source = new Map(leaves(en));
    const byLocale = new Map(translations.map(([locale, messages]) => [locale, new Map(leaves(messages))]));
    const stale: string[] = [];
    for (const [key, { locales }] of Object.entries(SAME_AS_ENGLISH)) {
      if (!source.has(key)) {
        stale.push(`${key}: no such key`);
        continue;
      }
      for (const locale of locales) {
        if (byLocale.get(locale)?.get(key) !== source.get(key)) stale.push(`${key} [${locale}]`);
      }
    }
    expect(stale).toEqual([]);
  });

  it("picks the phone's language, by region when it matters", () => {
    expect(pickLocale(["es-ES"])).toBe("es");
    expect(pickLocale(["pt-BR"])).toBe("pt");
    expect(pickLocale(["pt-PT"])).toBe("pt");
    expect(pickLocale(["zh-CN"])).toBe("zh-CN");
    expect(pickLocale(["zh-Hans-SG"])).toBe("zh-CN");
    expect(pickLocale(["zh-TW"])).toBe("zh-TW");
    expect(pickLocale(["zh-HK"])).toBe("zh-TW");
    expect(pickLocale(["zh-Hant"])).toBe("zh-TW");
    expect(pickLocale(["in-ID"])).toBe("id");
    expect(pickLocale(["AR-eg"])).toBe("ar");
  });

  it("walks the preferred languages in order and falls back to English", () => {
    expect(pickLocale(["xx-XX", "de-AT", "fr"])).toBe("de");
    expect(pickLocale(["xx", "yy"])).toBe("en");
    expect(pickLocale([])).toBe("en");
  });

  it("knows which languages read right to left", () => {
    expect(isRtl("ar")).toBe(true);
    expect(isRtl("es")).toBe(false);
    expect(isRtl("zh-TW")).toBe(false);
  });

  it("switches the texts, the document language and its direction", async () => {
    await setLocale("es");
    expect(i18n.global.locale.value).toBe("es");
    expect(t("tabs.settings")).not.toBe("Settings");
    expect(document.documentElement.lang).toBe("es");
    expect(document.documentElement.dir).toBe("ltr");

    await setLocale("ar");
    expect(document.documentElement.lang).toBe("ar");
    expect(document.documentElement.dir).toBe("rtl");

    await setLocale("en");
    expect(t("tabs.settings")).toBe("Settings");
    expect(document.documentElement.dir).toBe("ltr");
  });

  it("starts in the phone's language", async () => {
    const phone = Object.assign(new EventTarget(), { navigator: { languages: ["es-ES"], language: "es-ES" } });
    await followPhoneLanguage(phone);
    expect(i18n.global.locale.value).toBe("es");
  });

  // app#82 (2026-10-03): on Android a change of language no longer recreates the activity (that
  // left the old WebView alive), so the page switches its texts in place when the phone says so.
  it("follows a change of the phone's language while the app runs", async () => {
    const phone = Object.assign(new EventTarget(), { navigator: { languages: ["de-DE"], language: "de-DE" } });
    await followPhoneLanguage(phone);
    expect(i18n.global.locale.value).toBe("de");

    phone.navigator = { languages: ["ar-EG"], language: "ar-EG" };
    phone.dispatchEvent(new Event("languagechange"));
    await vi.waitFor(() => expect(i18n.global.locale.value).toBe("ar"));
    expect(document.documentElement.dir).toBe("rtl");
  });
});

// Seen on a phone in Spanish (app#73): an action-sheet select ends with Ionic's own "Cancel", in
// English whatever the language, unless the select names that button itself.
describe("option sheets", () => {
  const pages = import.meta.glob<string>("./**/*.vue", {
    eager: true,
    query: "?raw",
    import: "default",
  });
  // The opening tag of every select, attributes in quotes included (they may hold a `>`).
  const selects = Object.entries(pages).flatMap(([path, source]) =>
    [...source.matchAll(/<ion-select\b((?:"[^"]*"|'[^']*'|[^'">])*)>/g)].map(
      (match) => [path, match[1]] as const,
    ),
  );
  const sheets = selects.filter(([, attributes]) => /\binterface="action-sheet"/.test(attributes));

  it("finds the action-sheet selects of the app", () => {
    expect(sheets.length).toBeGreaterThan(0);
  });

  it("gives every action sheet a Cancel button in the phone's language", () => {
    const untranslated = sheets
      .filter(([, attributes]) => !attributes.includes(`:cancel-text="$t('common.cancel')"`))
      .map(([path]) => path);
    expect(untranslated).toEqual([]);
  });
});

// QA of 1.4.0 (2026-10-06): the send-later calendar started the week on Sunday in Spanish. The week
// starts where the phone's region says (0 is Sunday, as Ionic's picker counts), for the app's language.
describe("first day of the week", () => {
  afterEach(() => vi.restoreAllMocks());

  it("follows the phone's region for the app's language", () => {
    expect(firstDayOfWeek("es", ["es-ES"])).toBe(1);
    expect(firstDayOfWeek("es", ["es-MX"])).toBe(0);
    expect(firstDayOfWeek("en", ["en-US"])).toBe(0);
    expect(firstDayOfWeek("en", ["en-GB"])).toBe(1);
  });

  it("takes the language's usual region when the phone speaks another language", () => {
    expect(firstDayOfWeek("es", ["en-US"])).toBe(1);
    expect(firstDayOfWeek("de", [])).toBe(1);
    expect(firstDayOfWeek("en", [])).toBe(0);
    expect(firstDayOfWeek("ar", [])).toBe(6);
  });

  // The WebViews differ (2026-10-06): newer ones have `getWeekInfo()`, older ones a `weekInfo`
  // getter, the oldest neither. Each shape is stubbed on the prototype and put back afterwards, so
  // the test does not depend on what the runtime running it has.
  const proto = Intl.Locale.prototype as unknown as Record<string, unknown>;
  const KEYS = ["getWeekInfo", "weekInfo"] as const;
  function weekData(shape: Partial<Record<(typeof KEYS)[number], PropertyDescriptor>>): () => void {
    const saved = KEYS.map((key) => [key, Object.getOwnPropertyDescriptor(proto, key)] as const);
    for (const key of KEYS) Object.defineProperty(proto, key, shape[key] ?? { value: undefined, configurable: true });
    return () => {
      for (const [key, descriptor] of saved) {
        if (descriptor) Object.defineProperty(proto, key, descriptor);
        else Reflect.deleteProperty(proto, key);
      }
    };
  }

  it("knows the week without the browser's week data", () => {
    const restore = weekData({});
    try {
      expect(firstDayOfWeek("es", ["es-ES"])).toBe(1);
      expect(firstDayOfWeek("en", ["en-US"])).toBe(0);
      expect(firstDayOfWeek("pt", [])).toBe(0);
      expect(firstDayOfWeek("ar", ["ar-EG"])).toBe(6);
      expect(firstDayOfWeek("fr", ["fr-FR"])).toBe(1);
    } finally {
      restore();
    }
  });

  it("takes the browser's week data in either shape", () => {
    let restore = weekData({ getWeekInfo: { value: () => ({ firstDay: 6 }), configurable: true } });
    try {
      expect(firstDayOfWeek("es", ["es-ES"])).toBe(6);
    } finally {
      restore();
    }
    restore = weekData({ weekInfo: { get: () => ({ firstDay: 7 }), configurable: true } });
    try {
      expect(firstDayOfWeek("es", ["es-ES"])).toBe(0);
    } finally {
      restore();
    }
  });
});
