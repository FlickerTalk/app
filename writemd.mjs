import { chromium } from "playwright";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9334");
const page = browser.contexts()[0].pages()[0];
await page.evaluate(() => document.querySelector("[data-test='apps']")?.click());
await page.waitForTimeout(1000);
await page.evaluate(() => document.querySelector("[data-test='app-com.flickertalk.markdown']")?.click());
await page.waitForTimeout(3500);
console.log("frames:", page.frames().map((f) => f.url()));
const frame = page.frames().find((f) => f.url().includes("ftplugin"));
if (frame) {
  await frame.locator("textarea").fill("# Notes from yesterday\n\n- Bring the **signed** contract\n- Ask about the `invoice`\n\n> Meeting moved to 10:00");
  await frame.waitForTimeout(500);
  await frame.evaluate(() => document.querySelector("ft-markdown")?.shadowRoot?.querySelector("[data-act='look']")?.click());
  await page.waitForTimeout(1500);
}
await browser.close();
