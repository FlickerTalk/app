// Settings rows never cut their text (2026-10-02): on a 384 px Samsung in Spanish the receipts
// toggle and the auto-download and calls selects were clipped. At phone widths, in every
// language, a row's title, note and chosen value are read in full: a row that does not fit on one
// line grows instead. English, which fits, keeps its one-line rows. The same toggle and select
// rows on the contact, hours and circle info pages hold to it too.
import { readdirSync } from "node:fs";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

/** Every language the app ships: one catalogue each in `src/i18n`. */
const LOCALES = readdirSync(new URL("../src/i18n", import.meta.url))
  .filter((file) => file.endsWith(".json"))
  .map((file) => file.replace(/\.json$/, ""));

/**
 * Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test
 * e2e/settings-rows.spec.ts`.
 */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.screenshot({ path: `${dir}/${name}.png`, fullPage: true });
}

type Drawn = { text: string; lines: number; cut: string[]; brokenWord: boolean; splitSize: boolean };

/**
 * Every text drawn in the rows, with how many lines it takes and where it is cut: a line that
 * sticks out of a box hiding its overflow (Ionic's labels, the select's value, the item) or out of
 * its row, a single word split across lines because its box is narrower than the word, or a
 * size whose number and unit ("10 MB") land on different lines. It walks into the shadow roots, where Ionic draws labels and values, and climbs back
 * up through the slots, so a slotted title meets the boxes Ionic puts around it.
 */
function textsIn(rows: Element[]): Drawn[] {
  const clips = new Set(["hidden", "clip", "auto", "scroll"]);
  const texts: Drawn[] = [];
  const parentOf = (node: Node): Node | null => {
    const up = (node as Element | Text).assignedSlot ?? node.parentNode;
    return up instanceof ShadowRoot ? up.host : up;
  };
  const measure = (node: Text, row: Element) => {
    const range = document.createRange();
    range.selectNodeContents(node);
    const lines = [...range.getClientRects()].filter((line) => line.width > 0 && line.height > 0);
    if (!lines.length) return;
    const text = node.textContent!.trim();
    const cut: string[] = [];
    for (let box = parentOf(node); box; box = parentOf(box)) {
      if (!(box instanceof Element)) break;
      const style = getComputedStyle(box);
      const outer = box.getBoundingClientRect();
      const across = clips.has(style.overflowX);
      const down = clips.has(style.overflowY);
      const left = box === row || across ? outer.left + box.clientLeft : -Infinity;
      const right = box === row || across ? outer.left + box.clientLeft + box.clientWidth : Infinity;
      const top = down ? outer.top + box.clientTop : -Infinity;
      const bottom = down ? outer.top + box.clientTop + box.clientHeight : Infinity;
      for (const line of lines) {
        if (line.left < left - 1 || line.right > right + 1 || line.top < top - 1 || line.bottom > bottom + 1) {
          cut.push(`${box.tagName.toLowerCase()}.${[...box.classList].join(".")}`);
          break;
        }
      }
      if (box === row) break;
    }
    const count = new Set(lines.map((line) => Math.round(line.top))).size;
    // Chinese, Japanese and Thai break between letters, not at spaces: no word to split there.
    const oneWord = !/\s/.test(text) && !/[\p{sc=Han}\p{sc=Hiragana}\p{sc=Katakana}\p{sc=Thai}]/u.test(text);
    const splitSize = [...node.data.matchAll(/\d+(?:[.,]\d+)?\s+(?:B|KB|MB|GB|TB)\b/gu)].some((size) => {
      const part = document.createRange();
      part.setStart(node, size.index!);
      part.setEnd(node, size.index! + size[0].length);
      return new Set([...part.getClientRects()].filter((line) => line.width > 0).map((line) => Math.round(line.top))).size > 1;
    });
    texts.push({ text, lines: count, cut, brokenWord: count > 1 && oneWord, splitSize });
  };
  const visit = (node: Node, row: Element) => {
    if (node.nodeType === Node.TEXT_NODE) {
      if (node.textContent?.trim()) measure(node as Text, row);
      return;
    }
    if (!(node instanceof Element)) return;
    for (const child of node.shadowRoot?.childNodes ?? []) visit(child, row);
    for (const child of node.childNodes) visit(child, row);
  };
  for (const row of rows) visit(row, row);
  return texts;
}

/** Ionic's two looks: Android's (`md`) and the iPhone's (`ios`), chosen here by the URL. */
const MODES = ["md", "ios"] as const;
type Mode = (typeof MODES)[number];

/** Opens a page and waits until Ionic has drawn its controls, chosen values included, and the fonts are in. */
async function open(app: Page, path: string, root: string, mode: Mode) {
  await app.goto(`${path}?ionic:mode=${mode}`);
  await expect(app.locator(`${root} ion-item`).first()).toBeVisible();
  await expect(app.locator(`${root} ion-select:not(.hydrated), ${root} ion-toggle:not(.hydrated)`)).toHaveCount(0);
  for (const select of await app.locator(`${root} ion-select`).all()) await expect(select.locator(".select-text")).not.toBeEmpty();
  await app.evaluate(() => document.fonts.ready);
}

const openSettings = (app: Page, mode: Mode) => open(app, "/tabs/settings", ".ft-settings", mode);

/** Other pages with the same rows: a toggle with a title (and a note), a select with a label and a value. */
const OTHER_PAGES = [
  { name: "contact", path: "/contact/ft_bob123456789", root: ".ft-contact" },
  { name: "hours", path: "/hours", root: ".ft-hours" },
  { name: "circle info", path: "/circle/circle1/info", root: ".ft-circle" },
];

/** The phone widths: the Samsung where it was seen (384 px) and the narrowest common one (360 px). */
const WIDTHS = [360, 384];

/** The texts of the rows that are cut or split mid-word, at each width; the page loads once. */
async function cutAtEachWidth(app: Page, rows: Locator, shotName?: string): Promise<Record<number, Drawn[]>> {
  const found: Record<number, Drawn[]> = {};
  for (const width of WIDTHS) {
    await app.setViewportSize({ width, height: 800 });
    await app.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
    if (shotName && width === 384) await shot(app, shotName);
    const texts = await rows.evaluateAll(textsIn);
    expect(texts.length).toBeGreaterThan(0);
    found[width] = texts.filter((one) => one.cut.length || one.brokenWord || one.splitSize);
  }
  return found;
}

const NONE_CUT = Object.fromEntries(WIDTHS.map((width) => [width, []]));

for (const mode of MODES) {
  for (const locale of LOCALES) {
    test.describe(`${mode}, ${locale}`, () => {
      test.use({ locale });

      test(`no Settings row cuts its text at ${WIDTHS.join(" or ")} px`, async ({ app }) => {
        await openSettings(app, mode);
        const rows = app.locator(".ft-settings ion-list ion-item");
        expect(await rows.count()).toBeGreaterThan(10);
        const shotName = ["en", "es", "de", "ar"].includes(locale) ? `settings-${mode}-384-${locale}` : undefined;
        expect(await cutAtEachWidth(app, rows, shotName)).toEqual(NONE_CUT);
      });

      for (const page of OTHER_PAGES) {
        test(`no toggle or select row of the ${page.name} page cuts its text at ${WIDTHS.join(" or ")} px`, async ({ app }) => {
          await open(app, page.path, page.root, mode);
          const rows = app.locator(`${page.root} ion-item`).filter({ has: app.locator("ion-toggle, ion-select") });
          expect(await rows.count()).toBeGreaterThan(0);
          expect(await cutAtEachWidth(app, rows)).toEqual(NONE_CUT);
        });
      }
    });
  }

  test.describe(`${mode}, English at 384 px`, () => {
    test.use({ viewport: { width: 384, height: 800 }, locale: "en" });

    test("keeps every toggle title and note, and every select label and value, on one line", async ({ app }) => {
      await openSettings(app, mode);
      const texts = await app.locator(".ft-settings ion-item").filter({ has: app.locator("ion-toggle, ion-select") }).evaluateAll(textsIn);
      // Two toggles with a title and a note each, two selects with a label and a value each (the
      // tab bar's select went with the fixed Apps tab, 2026-10-08).
      expect(texts.length).toBe(8);
      expect(texts.filter((one) => one.lines !== 1)).toEqual([]);
    });
  });
}
