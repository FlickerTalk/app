import { createApp } from "vue";
import { IonicVue, isPlatform } from "@ionic/vue";
import App from "./App.vue";
import { router } from "./router";

import "@ionic/vue/css/core.css";
import "@ionic/vue/css/normalize.css";
import "@ionic/vue/css/structure.css";
import "@ionic/vue/css/typography.css";
import "@ionic/vue/css/padding.css";
import "@ionic/vue/css/flex-utils.css";
import "@ionic/vue/css/display.css";
// `ion-text-nowrap` and the other text utilities (2026-10-02: a long name cut in the apps sheet).
import "@ionic/vue/css/text-alignment.css";

import "./theme/variables.css";
import "./theme/base.css";
import { initTheme } from "./theme";
import { followPhoneLanguage, i18n, t } from "./i18n";
import { ionicConfig } from "./ionic";
import { enablePush, start } from "./core";
import { isOnboarded } from "./preferences";
import { loadHistory, startCalls } from "./calls";
import { startMoving } from "./moving";
import { startViewportFit } from "./viewport";

initTheme();
// The app is as tall as what the on-screen keyboard leaves visible (viewport.ts, theme/base.css).
startViewportFit(window, document.documentElement);

const app = createApp(App).use(i18n).use(router);

// The Rust core opens the identity and the local history before the first screen; without it
// (a plain browser during development) the UI still starts, empty.
const ready = start().then(async () => {
  await startCalls();
  await startMoving();
  // M4: tokens change; the router learns the current one at every start.
  if (isOnboarded()) void enablePush();
  await loadHistory();
});
// The texts follow the phone's language; the catalogue loads before the first screen.
const language = followPhoneLanguage();
Promise.allSettled([ready, language, router.isReady()]).then(() => {
  // Ionic reads its settings when installed, so it waits for the texts (the back button's word).
  app.use(IonicVue, ionicConfig(t("common.back"), isPlatform("ios")));
  app.mount("#app");
});
