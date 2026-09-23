import { chromium } from "playwright";
const browser = await chromium.connectOverCDP("http://127.0.0.1:9336");
const page = browser.contexts()[0].pages()[0];
const svg = await page.evaluate(() => document.querySelector(".ft-qr")?.innerHTML ?? "");
console.log("len:", svg.length);
console.log(svg.slice(0, 400));
await browser.close();
