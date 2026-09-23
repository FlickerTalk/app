import { chromium } from "playwright";
const [port, contact, ...words] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
if (!page.url().includes(contact)) {
  await page.goto(`http://tauri.localhost/chat/${contact}`);
  await page.waitForTimeout(2000);
}
await page.locator("ion-textarea textarea").first().fill(words.join(" "));
await page.waitForTimeout(300);
await page.evaluate(() => {
  const send = [...document.querySelectorAll("button")].find((b) => b.getAttribute("aria-label") === "Send");
  send?.click();
});
await page.waitForTimeout(1500);
await browser.close();
