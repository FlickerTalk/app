import { chromium } from "playwright";
const [port, name] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
if (page.url().includes("welcome")) {
  await page.locator("input").first().fill(name);
  await page.waitForTimeout(300);
  await page.evaluate(() => {
    const start = [...document.querySelectorAll("ion-button, button")].find((b) => /start/i.test(b.textContent ?? ""));
    start?.click();
  });
  await page.waitForTimeout(2500);
}
console.log("url:", page.url());
await browser.close();
