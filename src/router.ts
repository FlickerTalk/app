import { createRouter, createWebHistory } from "@ionic/vue-router";
import type { RouteLocationNormalized, RouteRecordRaw } from "vue-router";
import TabsPage from "./views/TabsPage.vue";
import { isOnboarded } from "./preferences";

export const routes: RouteRecordRaw[] = [
  { path: "/", redirect: "/tabs/chats" },
  { path: "/welcome", component: () => import("./views/WelcomePage.vue") },
  {
    path: "/tabs/",
    component: TabsPage,
    children: [
      { path: "", redirect: "/tabs/chats" },
      { path: "chats", component: () => import("./views/ChatsPage.vue") },
      { path: "calls", component: () => import("./views/CallsPage.vue") },
      // 2026-10-08 (plan of the apps grid): the tools and the games, one tab. The games and the
      // plugins had a tab each (Settings' choice, 2026-10-05); their addresses lead here now.
      { path: "apps", component: () => import("./views/AppsPage.vue") },
      { path: "games", redirect: "/tabs/apps" },
      { path: "plugins", redirect: "/tabs/apps" },
      { path: "settings", component: () => import("./views/SettingsPage.vue") },
    ],
  },
  // A conversation opens full screen, outside the tabs (phones). Wide screens show it next to the list.
  { path: "/chat/:id", component: () => import("./views/ChatPage.vue") },
  // Circles (2026-09-27): a conversation of many, its settings, and making a new one.
  { path: "/circle/:id", component: () => import("./views/CirclePage.vue") },
  { path: "/circle/:id/info", component: () => import("./views/CircleInfoPage.vue") },
  { path: "/new-circle", component: () => import("./views/NewCirclePage.vue") },
  { path: "/add-contact", component: () => import("./views/AddContactPage.vue") },
  { path: "/contact/:id", component: () => import("./views/ContactPage.vue") },
  { path: "/blocked", component: () => import("./views/BlockedPage.vue") },
  { path: "/session", component: () => import("./views/SessionPage.vue") },
  { path: "/hours", component: () => import("./views/HoursPage.vue") },
  // Settings → Plugins is gone (2026-10-08): its address opens the Apps tab.
  { path: "/plugins", redirect: "/tabs/apps" },
  // A plugin on its own (2026-09-27): from Settings, or from a reminder it set.
  { path: "/plugin/:id", component: () => import("./views/PluginPage.vue") },
  // The Plan screen is gone (2026-10-08): its address opens the Premium section of Settings.
  { path: "/plan", redirect: { path: "/tabs/settings", hash: "#premium" } },
  { path: "/move", component: () => import("./views/MovePage.vue") },
  // The user's own cloud (2026-09-27): the backup of this phone, and the drive behind it.
  { path: "/backup", component: () => import("./views/BackupPage.vue") },
  { path: "/call/:id", component: () => import("./views/CallPage.vue") },
];

// On first run the identity is created and shown before anything else (Plan §6), unless the
// identity comes from the old phone (§60).
const FIRST_RUN_PATHS = new Set(["/welcome", "/move"]);

export function onboardingGuard(path: string): string | true {
  return isOnboarded() || FIRST_RUN_PATHS.has(path) ? true : "/welcome";
}

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
});

router.beforeEach((to: RouteLocationNormalized) => onboardingGuard(to.path));
