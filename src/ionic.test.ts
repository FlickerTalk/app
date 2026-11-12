import { describe, expect, it } from "vitest";

import { ionicConfig } from "./ionic";
import { pageTransition } from "./page-transition";

describe("Ionic's settings", () => {
  it("gives iOS's back button the word of the phone's language", () => {
    expect(ionicConfig("رجوع", true)).toMatchObject({ backButtonText: "رجوع" });
  });

  it("lets Ionic pick the mode from the phone, as the plugins' frames do", () => {
    expect(ionicConfig("رجوع", true)).not.toHaveProperty("mode");
    expect(ionicConfig("رجوع", false)).not.toHaveProperty("mode");
  });

  it("leaves Android's back button as an arrow alone", () => {
    expect(ionicConfig("رجوع", false)).not.toHaveProperty("backButtonText");
  });

  it("uses the app's page transition on iOS and on Android", () => {
    expect(ionicConfig("رجوع", true).navAnimation).toBe(pageTransition);
    expect(ionicConfig("رجوع", false).navAnimation).toBe(pageTransition);
  });
});
