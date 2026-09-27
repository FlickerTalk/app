import { defineConfig, devices } from "@playwright/test";

// End-to-end tests of the web UI against a fake core (`e2e/fake-core.ts`): the real Vue app, the
// real `@tauri-apps/api`, and an in-memory stand-in behind `window.__TAURI_INTERNALS__`. They
// run on the Vite dev server; `npm run test:e2e`.
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: "http://localhost:1420",
    // The app marks what tests look for with `data-test`, as the unit tests do.
    testIdAttribute: "data-test",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: {
        ...devices["Pixel 7"],
        defaultBrowserType: "chromium",
        // A Chromium of your own (a CI image with one preinstalled, say); otherwise Playwright's.
        launchOptions: process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {},
      },
    },
  ],
  webServer: {
    command: "npx vite --port 1420 --strictPort",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
