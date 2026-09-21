// English is the source language. No translations in phase 1 (Plan §84), but every visible string
// goes through this catalogue so translating later does not touch the components.
import { createI18n } from "vue-i18n";
import en from "./i18n/en.json";

export const i18n = createI18n({
  legacy: false,
  globalInjection: true,
  locale: "en",
  fallbackLocale: "en",
  messages: { en },
});

export const t = i18n.global.t;
