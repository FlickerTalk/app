import { createApp } from "vue";
import { IonicVue } from "@ionic/vue";
import App from "./App.vue";
import { router } from "./router";

import "@ionic/vue/css/core.css";
import "@ionic/vue/css/normalize.css";
import "@ionic/vue/css/structure.css";
import "@ionic/vue/css/typography.css";
import "@ionic/vue/css/padding.css";
import "@ionic/vue/css/flex-utils.css";
import "@ionic/vue/css/display.css";

import "./theme/variables.css";
import "./theme/base.css";
import { initTheme } from "./theme";
import { i18n } from "./i18n";
import { enablePush, start } from "./core";
import { isOnboarded } from "./preferences";
import { loadHistory, startCalls } from "./calls";
import { startMoving } from "./moving";

initTheme();

const app = createApp(App).use(IonicVue).use(i18n).use(router);

// The Rust core opens the identity and the local history before the first screen; without it
// (a plain browser during development) the UI still starts, empty.
const ready = start().then(async () => {
  await startCalls();
  await startMoving();
  // M4: tokens change; the router learns the current one at every start.
  if (isOnboarded()) void enablePush();
  await loadHistory();
});
Promise.allSettled([ready, router.isReady()]).then(() => {
  app.mount("#app");
});
