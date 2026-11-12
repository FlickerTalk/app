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
startViewportFit(window, document.documentElement);

const app = createApp(App).use(i18n).use(router);

// without the Rust core (a plain browser in dev) the UI still starts, empty
const ready = start().then(async () => {
  await startCalls();
  await startMoving();
  // push tokens change, tell the router at every start
  if (isOnboarded()) void enablePush();
  await loadHistory();
});
const language = followPhoneLanguage();
Promise.allSettled([ready, language, router.isReady()]).then(() => {
  // Ionic reads its config when installed, so wait for the texts (the back button's word)
  app.use(IonicVue, ionicConfig(t("common.back"), isPlatform("ios")));
  app.mount("#app");
});
