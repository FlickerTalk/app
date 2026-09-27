/// <reference types="vitest/config" />
import { configDefaults, defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
// @ts-expect-error type error without @types/node package
import process from "node:process";
const host = process.env.TAURI_DEV_HOST;

// Community plugins are web components named `ft-*`: Vue must render them as custom elements.
const isPluginElement = (tag: string) => tag.startsWith("ft-");

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [vue({ template: { compilerOptions: { isCustomElement: isPluginElement } } })],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  test: {
    environment: "happy-dom",
    setupFiles: ["src/__tests__/setup.ts"],
    // The end-to-end specs are Playwright's (`npm run test:e2e`), not Vitest's.
    exclude: [...configDefaults.exclude, "e2e/**"],
    // Theme tests read the design tokens as text.
    css: { include: [/theme\/variables\.css/] },
  },
}));
