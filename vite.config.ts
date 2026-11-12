/// <reference types="vitest/config" />
import { configDefaults, defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

const isPluginElement = (tag: string) => tag.startsWith("ft-");

export default defineConfig(() => ({
  plugins: [vue({ template: { compilerOptions: { isCustomElement: isPluginElement } } })],

  // so Vite doesn't hide Rust errors
  clearScreen: false,
  // Tauri expects a fixed port
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "happy-dom",
    setupFiles: ["src/__tests__/setup.ts"],
    exclude: [...configDefaults.exclude, "e2e/**"],
    // the theme tests read these stylesheets as text
    css: { include: [/theme\/variables\.css/, /theme\/base\.css/] },
  },
}));
