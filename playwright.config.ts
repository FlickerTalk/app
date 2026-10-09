import { defineConfig, devices } from "@playwright/test";

// End-to-end tests of the web UI against a fake core (`e2e/fake-core.ts`): the real Vue app, the
// real `@tauri-apps/api`, and an in-memory stand-in behind `window.__TAURI_INTERNALS__`. They
// run on the Vite dev server; `npm run test:e2e`.
const chromium = {
  ...devices["Pixel 7"],
  defaultBrowserType: "chromium",
  // A Chromium of your own (a CI image with one preinstalled, say); otherwise Playwright's.
  launchOptions: process.env.PW_CHROMIUM ? { executablePath: process.env.PW_CHROMIUM } : {},
};

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
      grepInvert: /@timing/,
      use: chromium,
    },
    // What measures time on the wall clock (`@timing`, 2026-10-09), one test at a time; `npm run
    // test:e2e` runs it once the rest is done: on a CI runner, the other worker running the rest
    // of the suite slowed it past its bounds. Not a `dependencies` of `chromium`: that one would run whole, past
    // any file or title filter.
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
