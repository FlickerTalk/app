import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const game = (id: string, name: string, locales: Record<string, string>) => ({
  id: `com.flickertalk.game.${id}`,
  name,
  version: "1.0.0",
  summary: name,
  size: 40_000,
  carried: false,
  kind: "game",
  locales: Object.fromEntries(Object.entries(locales).map(([language, local]) => [language, { name: local }])),
});

const CATALOGUE = [
  game("balloonfair", "Balloon Fair", { de: "Ballonjahrmarkt", es: "Feria de globos", ar: "مهرجان البالونات" }),
  game("wordgrid", "Letter Grid", { de: "Buchstabengitter", es: "Cuadrícula de letras", ar: "شبكة الحروف" }),
  game("beatwagons", "Beat Wagons", { de: "Rhythmuswagen", es: "Vagones con ritmo", ar: "عربات الإيقاع" }),
  game("foamduel", "Foam Duel", { de: "Schaumstoffduell", es: "Duelo de espuma", ar: "مبارزةالإسفنجالطويلة" }),
  game("shardsplitter", "Shard Splitter", { de: "Kristallspalter", es: "Rompecristales", ar: "محطم البلورات" }),
  game("robotrumble", "Robot Rumble", { de: "Roboter-Rauferei", es: "Combate de robots", ar: "قتال الروبوتات" }),
  game("checkers", "Checkers", { de: "Dame", es: "Damas", ar: "الداما" }),
  game("fourinarow", "Four in a Row", { de: "Vier in einer Reihe", es: "Cuatro en raya", ar: "أربعة في صف" }),
  game("longest", "Longest", { de: "Donaudampfschifffahrtsgesellschaft", es: "Superextraordinariamentelarguísimo", ar: "الكلمةالأطولالتيلاتتسعأبداحتىبأصغرخط" }),
];
const SHORT = ["Dame", "Damas", "الداما", "Feria de globos", "Cuatro en raya"];
const CUT = new Set(["Donaudampfschifffahrtsgesellschaft", "Superextraordinariamentelarguísimo", "الكلمةالأطولالتيلاتتسعأبداحتىبأصغرخط"]);

type Laid = { name: string; label: string; size: number; clipped: boolean; cut: boolean; tile: number; split: string[]; outside: string[] };

async function names(app: Page, grid: string): Promise<Laid[]> {
  return app.getByTestId(grid).evaluate((root) =>
    [...root.querySelectorAll<HTMLElement>(".ft-app-tile")].map((tile) => {
      const name = tile.querySelector<HTMLElement>(".ft-app-tile__name")!;
      const text = name.firstChild as Text;
      const box = name.getBoundingClientRect();
      const split: string[] = [];
      const outside: string[] = [];
      for (const match of text.data.matchAll(/[^\s-]+-?/g)) {
        const range = document.createRange();
        range.setStart(text, match.index!);
        range.setEnd(text, match.index! + match[0].length);
        const tops = new Set([...range.getClientRects()].map((rect) => Math.round(rect.top)));
        if (tops.size > 1) split.push(match[0]);
        const word = range.getBoundingClientRect();
        if (word.left < box.left - 0.5 || word.right > box.right + 0.5) outside.push(match[0]);
      }
      const style = getComputedStyle(name);
      return {
        name: text.data,
        label: tile.getAttribute("aria-label") ?? "",
        size: parseFloat(style.fontSize),
        clipped: name.classList.contains("ft-app-tile__name--clipped"),
        cut: style.textOverflow === "ellipsis" && style.whiteSpace === "nowrap" && name.scrollWidth > name.clientWidth,
        tile: Math.round(tile.getBoundingClientRect().height),
        split,
        outside,
      };
    }),
  );
}

function checked(laid: Laid[]) {
  expect(laid.length).toBeGreaterThan(0);
  for (const one of laid) {
    expect(one.split, `${one.name} is split inside a word`).toEqual([]);
    expect(one.label, `${one.name} keeps its whole name for screen readers`).toContain(one.name);
    expect(one.size, `${one.name} stays readable`).toBeGreaterThanOrEqual(9);
    if (one.clipped) {
      expect(one.cut, `${one.name} ends with an ellipsis`).toBe(true);
      expect(one.size, `${one.name} is cut only at the smallest size`).toBe(9);
    } else {
      expect(one.outside, `${one.name} fits its tile`).toEqual([]);
    }
    if (CUT.has(one.name)) expect(one.clipped, `${one.name} is cut`).toBe(true);
    if (SHORT.includes(one.name)) expect([one.size, one.clipped], `${one.name} keeps the usual size`).toEqual([12, false]);
  }
  expect(new Set(laid.map((one) => one.tile)).size, "every tile in the grid has the same height").toBe(1);
  return laid.some((one) => !one.clipped && one.size < 12);
}

test.use({ viewport: { width: 360, height: 780 } });

test.beforeEach(async ({ app }) => {
  await app.addInitScript((entries) => {
    (window as unknown as Record<string, unknown>).__ftFakeCatalogue = entries;
  }, CATALOGUE);
});

for (const locale of ["de-DE", "es-ES", "ar"]) {
  test.describe(locale, () => {
    test.use({ locale });

    test("game names on a narrow phone are never split inside a word", async ({ app }) => {
      await app.goto("/tabs/apps?show=games");
      await expect(app.getByTestId("apps-more")).toBeVisible();
      for (const one of CATALOGUE.slice(0, 5)) {
        await app.getByTestId(`install-${one.id}`).click();
        await expect(app.getByTestId(`app-${one.id}`)).toBeVisible();
      }
      const shrunk = [checked(await names(app, "apps-installed")), checked(await names(app, "apps-more"))];
      if (locale !== "ar") expect(shrunk, "a name too wide for its tile is shrunk to fit").toContain(true);

      await app.locator(".ft-tab-bar ion-tab-button").first().click();
      await app.getByTestId("chat-row").filter({ hasText: "Bob" }).first().click();
      await app.getByTestId("apps").click();
      await app.getByTestId("apps-tab-games").click();
      await expect(app.getByTestId("games-sheet").locator(".ft-app-tile")).toHaveCount(6);
      checked(await names(app, "games-sheet"));
    });
  });
}
