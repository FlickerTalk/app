import js from "@eslint/js";
import { defineConfig, globalIgnores } from "eslint/config";
import vue from "eslint-plugin-vue";
import tseslint from "typescript-eslint";

export default defineConfig(
  globalIgnores(["dist/", "dist-ssr/", "coverage/", "test-results/", "playwright-report/", "target/", "src-tauri/"]),
  js.configs.recommended,
  tseslint.configs.recommended,
  vue.configs["flat/essential"],
  {
    files: ["**/*.vue"],
    languageOptions: { parserOptions: { parser: tseslint.parser } },
    // TypeScript already checks names, and this rule does not know the browser globals.
    rules: { "no-undef": "off" },
  },
  {
    rules: {
      "max-lines-per-function": ["error", { max: 100, skipBlankLines: true, skipComments: true }],
      "max-depth": ["error", 5],
      "max-params": ["error", 7],
    },
  },
  {
    files: ["**/*.test.ts", "src/__tests__/**", "e2e/**"],
    rules: { "max-lines-per-function": "off" },
  },
);
