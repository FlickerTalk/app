import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));

import { translateDetect, translateLanguages, translatePrepare, translateRemove, translateStatus, translateText } from "./translate";

const TRANSLATE = "com.flickertalk.translate";

describe("translation on this phone", () => {
  beforeEach(() => tauri.invoke.mockReset());

  it("asks the core, for the plugin, which languages there are and to translate", async () => {
    tauri.invoke
      .mockResolvedValueOnce(["en", "es"])
      .mockResolvedValueOnce("fr")
      .mockResolvedValueOnce("downloadable")
      .mockResolvedValueOnce(true)
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce("hola");
    expect(await translateLanguages(TRANSLATE)).toEqual(["en", "es"]);
    expect(await translateDetect(TRANSLATE, "bonjour")).toBe("fr");
    expect(await translateStatus(TRANSLATE, "fr", "es")).toBe("downloadable");
    expect(await translatePrepare(TRANSLATE, "fr", "es")).toBe(true);
    expect(await translateRemove(TRANSLATE, "fr", "es")).toBe(false);
    expect(await translateText(TRANSLATE, "hello", "en", "es")).toBe("hola");
    expect(tauri.invoke.mock.calls).toEqual([
      ["core_translate_languages", { plugin: TRANSLATE }],
      ["core_translate_detect", { plugin: TRANSLATE, text: "bonjour" }],
      ["core_translate_status", { plugin: TRANSLATE, from: "fr", to: "es" }],
      ["core_translate_prepare", { plugin: TRANSLATE, from: "fr", to: "es" }],
      ["core_translate_remove", { plugin: TRANSLATE, from: "fr", to: "es" }],
      ["core_translate_text", { plugin: TRANSLATE, text: "hello", from: "en", to: "es" }],
    ]);
  });

  it("lets the core's refusal reach the plugin", async () => {
    tauri.invoke.mockRejectedValueOnce("not_granted");
    await expect(translateText(TRANSLATE, "hello", "en", "es")).rejects.toBe("not_granted");
  });
});
