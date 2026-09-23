import { chromium } from "playwright";
const [port, link] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];

await page.goto("http://tauri.localhost/add-contact");
await page.waitForTimeout(1500);
await page.evaluate(() => document.querySelector("[data-test='mode-scan']")?.click());
await page.waitForTimeout(800);
await page.locator("[data-test='paste'] input, input[data-test='paste']").first().fill(link);
await page.waitForTimeout(500);
await page.evaluate(() => document.querySelector("[data-test='add']")?.click());
await page.waitForTimeout(4000);
console.log("url:", page.url(), "|", (await page.evaluate(() => document.body.innerText)).slice(0, 120).replace(/\n/g, " / "));
await browser.close();
