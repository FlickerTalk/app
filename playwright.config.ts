import { defineConfig, devices } from "@playwright/test";

const chromium = {
  ...devices["Pixel 7"],
  defaultBrowserType: "chromium",
  launchOptions: process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {},
};

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:1420",
    testIdAttribute: "data-test",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      grepInvert: /@timing/,
      use: chromium,
    },
    // @timing tests run alone after the rest, a parallel CI worker slowed them past their bounds.
    // Not a `dependencies` of chromium, that would run them whole whatever the filter.
    {
      name: "timing",
      grep: /@timing/,
      workers: 1,
      use: chromium,
    },
  ],
  webServer: {
    command: "npx vite --port 1420 --strictPort",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
