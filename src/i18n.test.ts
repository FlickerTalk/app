import { afterEach, describe, expect, it } from "vitest";
import { i18n, isRtl, LOCALES, pickLocale, setLocale, t } from "./i18n";
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

  describe.each(translations)("the %s catalogue", (_locale, messages) => {
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
});
