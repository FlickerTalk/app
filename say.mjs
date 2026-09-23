// Types a message in the open conversation and sends it.
import { chromium } from "playwright";
const [port, ...words] = process.argv.slice(2);
const text = words.join(" ");
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
await page.locator("ion-textarea textarea").first().fill(text);
await page.waitForTimeout(400);
await page.evaluate(() => {
  const send = [...document.querySelectorAll("button")].find((b) => b.getAttribute("aria-label") === "Send");
  send?.click();
});
await page.waitForTimeout(1200);
await browser.close();
