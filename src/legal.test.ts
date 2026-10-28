import { beforeEach, describe, expect, it, vi } from "vitest";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import { PRIVACY_URL, SUPPORT_ADDRESS, TERMS_URL, mailSupport, openOutside } from "./legal";

// App Review guideline 1.2 (2026-10-28): the terms the user accepts on Start, the privacy policy
// and a way to reach a person, all from inside the app. They live on the website.
describe("legal", () => {
  beforeEach(() => {
    opener.openUrl.mockReset();
  });

  it("points at the website's Terms of Use and Privacy Policy", () => {
    expect(TERMS_URL).toBe("https://flickertalk.com/terms/");
    expect(PRIVACY_URL).toBe("https://flickertalk.com/privacy/");
  });

  it("is reached at the same address the reports go to", () => {
    expect(SUPPORT_ADDRESS).toBe("info@flickertalk.com");
  });

  it("opens a page in the phone's browser, outside the app", async () => {
    await openOutside(TERMS_URL);
    expect(opener.openUrl).toHaveBeenCalledWith("https://flickertalk.com/terms/");
  });

  it("writes to support in the phone's mail app", async () => {
    await mailSupport();
    expect(opener.openUrl).toHaveBeenCalledWith("mailto:info@flickertalk.com");
  });

  // A phone with no browser or no mail app refuses: nothing breaks, the screen stays as it was.
  it("says nothing when the phone cannot open it", async () => {
    opener.openUrl.mockRejectedValue(new Error("no handler"));
    await expect(openOutside(PRIVACY_URL)).resolves.toBeUndefined();
    await expect(mailSupport()).resolves.toBeUndefined();
  });
});
