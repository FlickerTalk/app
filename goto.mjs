import { chromium } from "playwright";
const [port, where] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
await page.goto(`http://tauri.localhost/${where}`);
await page.waitForTimeout(2500);
await browser.close();
