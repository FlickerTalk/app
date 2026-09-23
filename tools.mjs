import { chromium } from "playwright";
const [port, what] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
await page.evaluate(() => document.querySelector("[data-test='apps']")?.click());
await page.waitForTimeout(1200);
if (what) {
  await page.evaluate((id) => document.querySelector(`[data-test='app-${id}']`)?.click(), what);
  await page.waitForTimeout(3000);
}
await browser.close();
