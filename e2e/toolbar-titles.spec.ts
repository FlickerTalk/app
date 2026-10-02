// Toolbar titles are never cut (2026-10-02, app#57): on a 384 px Samsung in Spanish, Settings →
// "Move to a new phone" read "Transferir a un teléfono nue…". At phone widths, in every language and
// in both of Ionic's looks, every page title from the catalogue is read in full: no ellipsis, no line
// hidden below the bar, nothing outside the toolbar and nothing under the toolbar's buttons. A title
// that does not fit on one line wraps and the bar grows; a title that fits keeps the bar as it was.
//
// Left out on purpose: titles that are not catalogue text. A plugin's or a game's name (the plugin
// page, the game room) comes from its package, and a contact's or a circle's name in a chat header
// is the user's data; neither is in the catalogue.
import { readFileSync, readdirSync } from "node:fs";
import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

/** Every language the app ships: one catalogue each in `src/i18n`. */
const LOCALES = readdirSync(new URL("../src/i18n", import.meta.url))
  .filter((file) => file.endsWith(".json"))
  .map((file) => file.replace(/\.json$/, ""));

/** Ionic's two looks: Android's (`md`) and the iPhone's (`ios`). */
const MODES = ["md", "ios"] as const;
type Mode = (typeof MODES)[number];

/**
 * An iPhone's user agent for the `ios` look: the app then sets the back button's text from the
 * catalogue, as it does on a real iPhone ("Atrás", "Zurück"), so the title meets a button of the
 * real width.
 */
const IPHONE =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";

/** The phone widths: the Samsung where it was seen (384 px) and the narrowest common one (360 px). */
const WIDTHS = [360, 384];

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";

/**
 * Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test
 * e2e/toolbar-titles.spec.ts`.
 */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.screenshot({ path: `${dir}/${name}.png` });
}

/** A screen with titles: how to reach it, and the part of the page whose titles are measured. */
type Screen = { name: string; open: (app: Page, mode: Mode) => Promise<Locator> };

async function page(app: Page, path: string, mode: Mode): Promise<Locator> {
  await app.goto(`${path}${path.includes("?") ? "&" : "?"}ionic:mode=${mode}`);
  const shown = app.locator(".ion-page:not(.ion-page-hidden)").filter({ has: app.locator("ion-title") }).last();
  await expect(shown.locator("ion-title").first()).toBeVisible();
  return shown;
}

/** Every screen of the app with a catalogue title, each `<ion-title>` in `src` by the file it is in. */
const SCREENS: Array<Screen & { file: string }> = [
  { file: "views/ChatsPage.vue", name: "chats", open: (app, mode) => page(app, "/tabs/chats", mode) },
  { file: "views/CallsPage.vue", name: "calls", open: (app, mode) => page(app, "/tabs/calls", mode) },
  { file: "views/GamesPage.vue", name: "games", open: (app, mode) => page(app, "/tabs/games", mode) },
  {
    file: "views/GamesPage.vue",
    name: "games, who to play with",
    open: async (app, mode) => {
      await page(app, "/tabs/games", mode);
      await app.getByTestId(`play-${TICTACTOE}`).click();
      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
      return app.locator("ion-modal").filter({ has: app.getByTestId("contact-picker") });
    },
  },
  { file: "views/SettingsPage.vue", name: "settings", open: (app, mode) => page(app, "/tabs/settings", mode) },
  {
    file: "components/FeedbackModal.vue",
    name: "settings, suggest something",
    open: async (app, mode) => {
      await page(app, "/tabs/settings", mode);
      await app.getByTestId("feedback").click();
      const modal = app.getByTestId("feedback-modal");
      await expect(modal.getByTestId("send")).toBeInViewport({ ratio: 1 });
      return modal;
    },
  },
  { file: "views/AddContactPage.vue", name: "add contact", open: (app, mode) => page(app, "/add-contact", mode) },
  { file: "views/BackupPage.vue", name: "backup", open: (app, mode) => page(app, "/backup", mode) },
  { file: "views/BlockedPage.vue", name: "blocked", open: (app, mode) => page(app, "/blocked", mode) },
  { file: "views/HoursPage.vue", name: "hours", open: (app, mode) => page(app, "/hours", mode) },
  { file: "views/MovePage.vue", name: "move to a new phone", open: (app, mode) => page(app, "/move", mode) },
  { file: "views/MovePage.vue", name: "move from the old phone", open: (app, mode) => page(app, "/move?role=new", mode) },
  { file: "views/NewCirclePage.vue", name: "new circle", open: (app, mode) => page(app, "/new-circle", mode) },
  { file: "views/PlanPage.vue", name: "plan", open: (app, mode) => page(app, "/plan", mode) },
  { file: "views/PluginsPage.vue", name: "plugins", open: (app, mode) => page(app, "/plugins", mode) },
];

/** Titles that are not catalogue text (see the top of this file), by the file they are in. */
const NOT_CATALOGUE = ["components/ChatThread.vue", "views/PluginPage.vue"];

/** Where a title sits: a page's bar, the iPhone's large title under it, or a sheet's bar. */
type Place = "bar" | "large" | "sheet";
/** `offCentre`: how far the middle of the text is from the middle of its bar, in px. */
type Title = { text: string; place: Place; lines: number; bar: number; offCentre: number; cut: string[] };

/**
 * Every title drawn in `roots`, with how many lines it takes and how it is cut: its text box (Ionic's
 * `.toolbar-title`, in the shadow root) hides text across (the ellipsis) or below, a line of it
 * stands outside the toolbar's box (which hides what overflows), or a line runs under one of the
 * toolbar's buttons.
 */
function titlesIn(roots: Element[]): Title[] {
  const found: Title[] = [];
  const overlaps = (a: DOMRect, b: DOMRect) => a.left < b.right - 1 && a.right > b.left + 1 && a.top < b.bottom - 1 && a.bottom > b.top + 1;
  for (const root of roots) {
    for (const title of root.querySelectorAll("ion-title")) {
      if (!title.getClientRects().length) continue;
      const text = title.textContent!.trim();
      const range = document.createRange();
      range.selectNodeContents(title);
      const lines = [...range.getClientRects()].filter((line) => line.width > 0 && line.height > 0);
      if (!lines.length) continue;
      const cut: string[] = [];
      const box = title.shadowRoot!.querySelector(".toolbar-title") as HTMLElement;
      if (box.scrollWidth > box.clientWidth + 1) cut.push("ellipsis");
      if (box.scrollHeight > box.clientHeight + 1) cut.push("hidden below");
      const toolbar = title.closest("ion-toolbar")!;
      const bar = toolbar.shadowRoot!.querySelector(".toolbar-container")!.getBoundingClientRect();
      if (lines.some((line) => line.left < bar.left - 1 || line.right > bar.right + 1 || line.top < bar.top - 1 || line.bottom > bar.bottom + 1)) {
        cut.push("outside the toolbar");
      }
      for (const button of toolbar.querySelectorAll("ion-button, ion-back-button")) {
        const place = button.getBoundingClientRect();
        if (!place.width || getComputedStyle(button).display === "none") continue;
        if (lines.some((line) => overlaps(line, place))) cut.push(`under ${button.tagName.toLowerCase()}`);
      }
      found.push({
        text,
        place: title.classList.contains("title-large") ? "large" : title.closest("ion-modal.modal-sheet") ? "sheet" : "bar",
        lines: new Set(lines.map((line) => Math.round(line.top))).size,
        bar: Math.round(toolbar.getBoundingClientRect().height * 10) / 10,
        offCentre: Math.round(
          (Math.min(...lines.map((line) => line.left)) + Math.max(...lines.map((line) => line.right))) / 2 -
            (toolbar.getBoundingClientRect().left + toolbar.getBoundingClientRect().right) / 2,
        ),
        cut,
      });
    }
  }
  return found;
}

/** Lets the layout settle after a resize: two frames. */
const settle = (app: Page) => app.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));

test("every toolbar title in the app is either measured here or not catalogue text", () => {
  const source = new URL("../src/", import.meta.url);
  const files = readdirSync(source, { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".vue") && readFileSync(new URL(file, source), "utf8").includes("<ion-title"))
    .sort();
  const covered = new Set([...SCREENS.map((screen) => screen.file), ...NOT_CATALOGUE]);
  expect(files.filter((file) => !covered.has(file))).toEqual([]);
});

for (const mode of MODES) {
  for (const locale of LOCALES) {
    test.describe(`${mode}, ${locale}`, () => {
      test.use({ locale, ...(mode === "ios" ? { userAgent: IPHONE } : {}) });

      test(`no toolbar title is cut at ${WIDTHS.join(" or ")} px`, async ({ app }) => {
        test.setTimeout(120_000);
        const cut: string[] = [];
        for (const screen of SCREENS) {
          await app.setViewportSize({ width: WIDTHS[WIDTHS.length - 1], height: 800 });
          const root = await screen.open(app, mode);
          await app.evaluate(() => document.fonts.ready);
          for (const width of WIDTHS) {
            await app.setViewportSize({ width, height: 800 });
            await settle(app);
            const titles = await root.evaluateAll(titlesIn);
            expect(titles.length, `${screen.name} has a title`).toBeGreaterThan(0);
            for (const title of titles.filter((one) => one.cut.length)) {
              cut.push(`${screen.name} @ ${width}: "${title.text}" (${title.lines} line(s)) ${title.cut.join(", ")}`);
            }
          }
          if (["en", "es", "de", "fr", "ru", "uk", "ar", "ja"].includes(locale)) {
            await app.setViewportSize({ width: 384, height: 800 });
            await settle(app);
            await shot(app, `${screen.name.replace(/[^a-z]+/g, "-")}-${mode}-384-${locale}`);
          }
        }
        expect(cut).toEqual([]);
      });
    });
  }

  test.describe(`${mode}, short titles`, () => {
    test.use({ locale: "en", viewport: { width: 384, height: 800 }, ...(mode === "ios" ? { userAgent: IPHONE } : {}) });

    test("a title that fits stays on one line, where Ionic puts it, and the bar keeps Ionic's height", async ({ app }) => {
      // Ionic's heights, as they were before titles could wrap: in `md` every bar is 56 px; in
      // `ios` a page's bar is 44 px and a sheet's (its grabber above) 56 px. The iPhone's large title
      // has no fixed height (one line of its 34 px font: 51 px with Linux's fonts, 52 px with the
      // Mac's), so its bar is compared with the same bar holding a single letter.
      const heights: Record<Exclude<Place, "large">, number> = mode === "md" ? { bar: 56, sheet: 56 } : { bar: 44, sheet: 56 };
      for (const screen of SCREENS) {
        const root = await screen.open(app, mode);
        await app.evaluate(() => document.fonts.ready);
        const titles = await root.evaluateAll(titlesIn);
        expect(titles.filter((one) => one.lines !== 1 || one.cut.length), screen.name).toEqual([]);
        expect(titles.filter((one) => one.place !== "large" && one.bar !== heights[one.place]), screen.name).toEqual([]);
        for (const large of await root.locator("ion-title.title-large .ft-title").all()) {
          const [now, oneLetter] = await large.evaluate((text) => {
            const bar = text.closest("ion-toolbar")!;
            const now = bar.getBoundingClientRect().height;
            const was = text.textContent;
            text.textContent = "A";
            const oneLetter = bar.getBoundingClientRect().height;
            text.textContent = was;
            return [now, oneLetter];
          });
          expect(now, `${screen.name}, large title`).toBe(oneLetter);
        }
        // The iPhone's titles stay in the middle of their bar, where Ionic puts them.
        if (mode === "ios") expect(titles.filter((one) => one.place !== "large" && Math.abs(one.offCentre) > 1), screen.name).toEqual([]);
      }
    });
  });

  test.describe(`${mode}, under the status bar`, () => {
    test.use({ locale: "es", viewport: { width: 384, height: 800 }, ...(mode === "ios" ? { userAgent: IPHONE } : {}) });

    test("a long title still starts below the status bar or the dynamic island", async ({ app }) => {
      const inset = 47;
      await app.addInitScript((top) => {
        document.addEventListener("DOMContentLoaded", () => document.documentElement.style.setProperty("--ion-safe-area-top", `${top}px`));
      }, inset);
      const root = await page(app, "/move", mode);
      await app.evaluate(() => document.fonts.ready);
      const title = root.locator("ion-header ion-title").first();
      const top = await title.evaluate((one) => {
        const range = document.createRange();
        range.selectNodeContents(one);
        return Math.min(...[...range.getClientRects()].filter((line) => line.height > 0).map((line) => line.top));
      });
      expect(top).toBeGreaterThanOrEqual(inset);
      expect(await root.evaluateAll(titlesIn)).toEqual([expect.objectContaining({ cut: [] })]);
    });
  });
}
