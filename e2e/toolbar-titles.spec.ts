import { readFileSync, readdirSync } from "node:fs";
import type { Locator, Page } from "@playwright/test";
import { expect, frameSays, test } from "./helpers";

const LOCALES = readdirSync(new URL("../src/i18n", import.meta.url))
  .filter((file) => file.endsWith(".json"))
  .map((file) => file.replace(/\.json$/, ""));

const MODES = ["md", "ios"] as const;
type Mode = (typeof MODES)[number];

const IPHONE =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";

const WIDTHS = [360, 384];

const BOB = "ft_bob123456789";
const TICTACTOE = "com.flickertalk.game.tictactoe";
const MARKDOWN = "com.flickertalk.markdown";

async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.screenshot({ path: `${dir}/${name}.png` });
}

type Screen = { name: string; open: (app: Page, mode: Mode) => Promise<Locator> };

async function page(app: Page, path: string, mode: Mode): Promise<Locator> {
  await app.goto(`${path}${path.includes("?") ? "&" : "?"}ionic:mode=${mode}`);
  const shown = app.locator(".ion-page:not(.ion-page-hidden)").filter({ has: app.locator("ion-title") }).last();
  await expect(shown.locator("ion-title").first()).toBeVisible();
  return shown;
}

const SCREENS: Array<Screen & { file: string }> = [
  { file: "views/ChatsPage.vue", name: "chats", open: (app, mode) => page(app, "/tabs/chats", mode) },
  { file: "views/CallsPage.vue", name: "calls", open: (app, mode) => page(app, "/tabs/calls", mode) },
  { file: "views/AppsPage.vue", name: "apps", open: (app, mode) => page(app, "/tabs/apps", mode) },
  {
    file: "components/ContactPicker.vue",
    name: "apps, who to play with",
    open: async (app, mode) => {
      await page(app, "/tabs/apps?show=games", mode);
      await app.getByTestId(`app-${TICTACTOE}`).click();
      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId(`play-with-${BOB}`)).toBeVisible();
      return app.locator("ion-modal").filter({ has: app.getByTestId("contact-picker") });
    },
  },
  {
    file: "components/ContactPicker.vue",
    name: "a tool on its own, who to send to",
    open: async (app, mode) => {
      await page(app, `/plugin/${MARKDOWN}`, mode);
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await frameSays(app, { type: "ft.text", text: "# Title" });
      await expect(app.getByTestId(`send-to-${BOB}`)).toBeVisible();
      return app.locator("ion-modal").filter({ has: app.getByTestId(`send-to-${BOB}`) });
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
];

const NOT_CATALOGUE = ["components/PluginWindow.vue", "components/HookFrames.vue", "views/PluginPage.vue"];

type Place = "bar" | "large" | "sheet";
type Title = { text: string; place: Place; lines: number; bar: number; offCentre: number; cut: string[] };

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

const settle = (app: Page) => app.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));

test("every toolbar title in the app is either measured here or not catalogue text", () => {
  const source = new URL("../src/", import.meta.url);
  const files = readdirSync(source, { recursive: true, encoding: "utf8" })
    .filter((file) => file.endsWith(".vue") && readFileSync(new URL(file, source), "utf8").includes("<ion-title"))
    .sort();
  const covered = new Set([...SCREENS.map((screen) => screen.file), ...NOT_CATALOGUE]);
  expect(files.filter((file) => !covered.has(file))).toEqual([]);
});

test("the iPhone's title rules only apply where the browser knows :has()", async ({ app }) => {
  await app.goto("/tabs/settings");
  await expect(app.locator("ion-title").first()).toBeVisible();
  const outside = await app.evaluate(() => {
    const found: string[] = [];
    const walk = (rules: CSSRuleList) => {
      for (const rule of rules) {
        if (rule instanceof CSSSupportsRule && rule.conditionText.includes(":has(")) continue;
        if (rule instanceof CSSGroupingRule) walk(rule.cssRules);
        else if (rule instanceof CSSStyleRule && /ion-toolbar\.ios|ion-toolbar[^,]*:has\(|^(?!.*ion-toolbar\.md).*\.ft-title/.test(rule.selectorText)) {
          found.push(rule.selectorText);
        }
      }
    };
    for (const sheet of document.styleSheets) walk(sheet.cssRules);
    return found;
  });
  expect(outside).toEqual([]);
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
