import { beforeEach, describe, expect, it, vi } from "vitest";

const opener = vi.hoisted(() => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => opener);

import { PRIVACY_URL, SUPPORT_ADDRESS, TERMS_URL, mailSupport, openOutside } from "./legal";

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

  it("says nothing when the phone cannot open it", async () => {
    opener.openUrl.mockRejectedValue(new Error("no handler"));
    await expect(openOutside(PRIVACY_URL)).resolves.toBeUndefined();
    await expect(mailSupport()).resolves.toBeUndefined();
  });
});
