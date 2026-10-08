import { describe, expect, it } from "vitest";

// Seen in the UI review (2026-10-02): on iOS, Ionic writes its own "Back" beside the arrow of every
// back button, in English whatever the phone's language. The app gives it its own word instead.
import { ionicConfig } from "./ionic";
import { pageTransition } from "./page-transition";

describe("Ionic's settings", () => {
  it("gives iOS's back button the word of the phone's language", () => {
    expect(ionicConfig("رجوع", true)).toMatchObject({ backButtonText: "رجوع" });
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
