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

interface SameAsEnglish {
  locales: readonly Locale[];
  reason: string;
}

const ALL_TRANSLATED = LOCALES.filter((locale) => locale !== "en");
const LATIN_LOANWORDS = ["de", "es", "fr", "id", "it", "pl", "pt", "ro", "tr", "vi"] as const;
const SAME_WORD = "the same word (or an established loanword) in that language";
const THEME_NAME = "a theme name, kept as a proper name or the same word";

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
  "contact.days": {
    locales: ["es", "pt"],
    reason: "unit abbreviation shared with English",
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
  "common.ok": {
    locales: ["de", "fr", "id", "it", "ja", "pl", "pt", "ro", "vi"],
    reason: '"OK" is the word these languages put on a button',
  },
  "emoji.open": { locales: LATIN_LOANWORDS, reason: '"Emoji" is the word in that language' },
  "contact.reasons.spam": { locales: LATIN_LOANWORDS, reason: '"Spam" is the word in that language' },
  "colors.mono": { locales: LATIN_LOANWORDS, reason: THEME_NAME },
  "colors.aurora": { locales: ["de", "es", "id", "it", "pt", "tr", "vi"], reason: THEME_NAME },
  "colors.lime": { locales: ["id", "it", "ro", "vi"], reason: THEME_NAME },
  "colors.ember": { locales: ["id", "vi"], reason: THEME_NAME },
  "calls.video": { locales: ["de", "id", "it", "ro", "vi"], reason: SAME_WORD },
  "contact.video": { locales: ["de", "id", "it", "ro", "vi"], reason: SAME_WORD },
  "tabs.apps": { locales: ["de", "es", "fr", "pt"], reason: SAME_WORD },
  "premium.title": { locales: ["de", "es", "fr", "id", "it", "pl", "pt", "ro", "tr", "vi"], reason: SAME_WORD },
  "chat.edit": { locales: ["id"], reason: SAME_WORD },
  "settings.backup": { locales: ["de", "it", "pt", "ro"], reason: SAME_WORD },
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

describe("counts", () => {
  const entry = (locale: Locale, key: string) => String(new Map(leaves(catalogues[`./i18n/${locale}.json`])).get(key));
  const filled = (text: string, values: Record<string, number>) => text.replace(/\{(\w+)\}/g, (_, name: string) => String(values[name]));

  const NEVER_ONE: Record<string, string> = { "backup.phraseRule": "always 12 to 100 characters" };

  it("never put an English number straight before a plural noun, which reads wrong for one", () => {
    const wrong = leaves(en)
      .filter(([key]) => !(key in NEVER_ONE))
      .filter(([, text]) => /\{\w+\} (days|hours|minutes|seconds|weeks|months|years|messages|files|members|people|contacts|tries|times|characters)\b/.test(String(text)))
      .map(([key, text]) => `${key}: ${String(text)}`);
    expect(wrong).toEqual([]);
  });

  it("tell the last day of the free trial in correct words in every language", () => {
    const lastDay = Object.fromEntries(LOCALES.map((locale) => [locale, filled(entry(locale, "premium.daysLeft"), { days: 1 })]));
    expect(lastDay).toEqual({
      ar: "الأيام المتبقية: 1",
      bn: "1 দিন বাকি",
      de: "verbleibende Tage: 1",
      en: "days left: 1",
      es: "días restantes: 1",
      fr: "jours restants : 1",
      hi: "1 दिन बाकी",
      id: "sisa 1 hari",
      it: "giorni rimasti: 1",
      ja: "残り1日",
      ko: "1일 남음",
      pl: "pozostałe dni: 1",
      pt: "dias restantes: 1",
      ro: "zile rămase: 1",
      ru: "осталось дней: 1",
      th: "เหลือ 1 วัน",
      tr: "1 gün kaldı",
      uk: "залишилося днів: 1",
      vi: "còn 1 ngày",
      "zh-CN": "还剩 1 天",
      "zh-TW": "還剩 1 天",
    });
  });

  it("say the passphrase length in Romanian with the \"de\" that 100 needs", () => {
    expect(filled(entry("ro", "backup.phraseRule"), { min: 12, max: 100 })).toBe("Între 12 și 100 de caractere");
  });
});

describe("the Spanish catalogue", () => {
  it("writes \"vídeo\" with its accent", () => {
    const unaccented = leaves(catalogues["./i18n/es.json"])
      .filter(([, value]) => /\bvideos?\b/i.test(String(value)))
      .map(([key, value]) => `${key}: ${String(value)}`);
    expect(unaccented).toEqual([]);
  });
});

describe("option sheets", () => {
  const pages = import.meta.glob<string>("./**/*.vue", {
    eager: true,
    query: "?raw",
    import: "default",
  });
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
