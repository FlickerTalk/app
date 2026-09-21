import { createRouter, createWebHistory } from "@ionic/vue-router";
import type { RouteRecordRaw } from "vue-router";
import TabsPage from "./views/TabsPage.vue";

export const routes: RouteRecordRaw[] = [
  { path: "/", redirect: "/tabs/chats" },
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
];

export const router = createRouter({
  history: createWebHistory(import.meta.env.BASE_URL),
  routes,
});
