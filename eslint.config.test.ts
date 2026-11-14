import { ESLint } from "eslint";
import { beforeAll, expect, it } from "vitest";

const eslint = new ESLint();

beforeAll(() => eslint.lintText("", { filePath: "src/warm-up.ts" }), 30_000);

const rulesHit = async (code: string, filePath: string) => {
  const [result] = await eslint.lintText(code, { filePath });
  return result.messages.map((message) => message.ruleId);
};

const longFunction = `export function long() {\n  let n = 0;\n${"  n += 1;\n".repeat(120)}  return n;\n}\n`;

it("limits function length in app code but not in tests", async () => {
  expect(await rulesHit(longFunction, "src/long.ts")).toEqual(["max-lines-per-function"]);
  expect(await rulesHit(longFunction, "src/long.test.ts")).toEqual([]);
  expect(await rulesHit(longFunction, "src/__tests__/seed.ts")).toEqual([]);
  expect(await rulesHit(longFunction, "e2e/long.spec.ts")).toEqual([]);
});

it("reads TypeScript in Vue files and leaves browser globals to TypeScript", async () => {
  const component = `<script setup lang="ts">\nconst width: any = window.innerWidth;\nconsole.log(width);\n</script>\n`;
  expect(await rulesHit(component, "src/components/WindowWidth.vue")).toEqual(["@typescript-eslint/no-explicit-any"]);
});

it("accepts the native slot attribute that Ionic components use", async () => {
  const component = `<template>\n  <ion-item>\n    <ion-icon slot="start" />\n  </ion-item>\n</template>\n`;
  expect(await rulesHit(component, "src/components/StartIcon.vue")).toEqual([]);
});

it("lets names that start with an underscore go unused, and only those", async () => {
  const underscored = `export function pick(first: number, _second: number) {\n  const _unused = first;\n  try {\n    return first;\n  } catch (_error) {\n    return 0;\n  }\n}\n`;
  expect(await rulesHit(underscored, "src/pick.ts")).toEqual([]);
  expect(await rulesHit(underscored.replaceAll("_", ""), "src/pick.ts")).toEqual(
    Array(3).fill("@typescript-eslint/no-unused-vars"),
  );
});

it("does not lint the native side or build output", async () => {
  expect(await eslint.isPathIgnored("src-tauri/src/frame.js")).toBe(true);
  expect(await eslint.isPathIgnored("dist/assets/index.js")).toBe(true);
});
