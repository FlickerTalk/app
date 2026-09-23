import { chromium } from "playwright";
const [port] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
await page.goto("http://tauri.localhost/tabs/chats");
await page.waitForTimeout(3000);
console.log(page.url(), "|", (await page.evaluate(() => document.body.innerText)).slice(0, 200).replace(/\n/g, " / "));
await browser.close();
