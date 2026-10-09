import { describe, expect, it } from "vitest";

// Seen in the UI review (2026-10-02): on iOS, Ionic writes its own "Back" beside the arrow of every
// back button, in English whatever the phone's language. The app gives it its own word instead.
import { ionicConfig } from "./ionic";
import { pageTransition } from "./page-transition";

describe("Ionic's settings", () => {
  it("gives iOS's back button the word of the phone's language", () => {
    expect(ionicConfig("رجوع", true)).toMatchObject({ backButtonText: "رجوع" });
  });

  // 2026-10-09: the Ionic lent to the plugins' frames picks ios or md from the user agent, which the
  // frame shares with the app. That is the same as the app's only while the app forces no mode; if
  // it ever does, the frame has to be told (`frame.html`, `<html mode>`).
  it("lets Ionic pick the mode from the phone, as the plugins' frames do", () => {
    expect(ionicConfig("رجوع", true)).not.toHaveProperty("mode");
    expect(ionicConfig("رجوع", false)).not.toHaveProperty("mode");
  });

  // Android's (Material) back button is an arrow alone; it stays so.
  it("leaves Android's back button as an arrow alone", () => {
    expect(ionicConfig("رجوع", false)).not.toHaveProperty("backButtonText");
  });

  // 2026-10-09: Ionic's own transitions were slow (540 ms on iOS); the app's is the same on both.
  it("uses the app's page transition on iOS and on Android", () => {
    expect(ionicConfig("رجوع", true).navAnimation).toBe(pageTransition);
    expect(ionicConfig("رجوع", false).navAnimation).toBe(pageTransition);
  });
});
