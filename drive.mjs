// Reads the Contact Card link out of the QR the app is showing: the SVG is the module grid, so
// it is rebuilt into an image and decoded. Only for driving the emulators.
import { chromium } from "playwright";
import jsQR from "jsqr";

const [port] = process.argv.slice(2);
const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
const page = browser.contexts()[0].pages()[0];

if (!page.url().includes("add-contact")) {
  await page.goto("http://tauri.localhost/add-contact");
  await page.waitForTimeout(2000);
}
const svg = await page.evaluate(() => document.querySelector(".ft-qr")?.innerHTML ?? "");
await browser.close();

const size = Number(/viewBox="0 0 (\d+)/.exec(svg)?.[1] ?? 0);
if (!size) {
  console.error("no QR on screen");
  process.exit(1);
}
// The library draws one stroked line per row: `M<x> <y>.5h<run>m<gap> 0h<run>…`.
const dark = new Set();
for (const row of svg.split("M").slice(2)) {
  const start = /^(\d+) (\d+)\.5/.exec(row);
  if (!start) continue;
  let x = Number(start[1]);
  const y = Number(start[2]);
  for (const [, what, value] of row.matchAll(/([hm])(\d+)/g)) {
    if (what === "h") {
      for (let step = 0; step < Number(value); step += 1) dark.add(`${x + step},${y}`);
      x += Number(value);
    } else {
      x += Number(value);
    }
  }
}

const SCALE = 6;
const side = size * SCALE;
const data = new Uint8ClampedArray(side * side * 4).fill(255);
for (const module of dark) {
  const [x, y] = module.split(",").map(Number);
  for (let dy = 0; dy < SCALE; dy += 1) {
    for (let dx = 0; dx < SCALE; dx += 1) {
      const at = ((y * SCALE + dy) * side + (x * SCALE + dx)) * 4;
      data[at] = data[at + 1] = data[at + 2] = 0;
    }
  }
}
const read = jsQR(data, side, side);
console.log(read ? read.data : "could not read the code");
