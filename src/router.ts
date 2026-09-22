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
      { path: "settings", component: () => import("./views/SettingsPage.vue") },
    ],
  },
  // A conversation opens full screen, outside the tabs (phones). Wide screens show it next to the list.
  { path: "/chat/:id", component: () => import("./views/ChatPage.vue") },
  { path: "/add-contact", component: () => import("./views/AddContactPage.vue") },
  { path: "/contact/:id", component: () => import("./views/ContactPage.vue") },
  { path: "/blocked", component: () => import("./views/BlockedPage.vue") },
  { path: "/call/:id", component: () => import("./views/CallPage.vue") },
  // PoC 0 developer screen (Plan §87); reachable from Settings in development builds.
  { path: "/poc", component: () => import("./views/PocPage.vue") },
];

// On first run the identity is created and shown before anything else (Plan §6).
export function onboardingGuard(path: string): string | true {
  return isOnboarded() || path === "/welcome" ? true : "/welcome";
}

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
});

router.beforeEach((to: RouteLocationNormalized) => onboardingGuard(to.path));
