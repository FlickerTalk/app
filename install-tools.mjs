// Installs a couple of tools from the catalogue, the way a user would.
import { chromium } from "playwright";
const [port, ...wanted] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];
await page.goto("http://tauri.localhost/plugins");
await page.waitForTimeout(4000);
for (const id of wanted) {
  const done = await page.evaluate((one) => {
    const button = document.querySelector(`[data-test='install-${one}']`);
    button?.click();
    return Boolean(button);
  }, id);
  console.log(id, done ? "installing" : "not offered");
  await page.waitForTimeout(4000);
}
console.log((await page.evaluate(() => document.body.innerText)).replace(/\n/g, " / ").slice(0, 300));
await browser.close();
