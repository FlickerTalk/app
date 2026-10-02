// 2026-10-02 (plan of the catalogue's translations): a Spanish phone names plugins and games in
// Spanish, from their package or, when the package installed here has none, from the catalogue's
// entry; summaries too. The fake core has Sketch and Tic-tac-toe installed without translations.
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";
const CHESS = "com.flickertalk.game.chess";

/** What the catalogue lists besides Chess, in Spanish too; two of them are installed here. */
const CATALOGUE = [
  {
    id: "com.flickertalk.list",
    name: "List",
    version: "1.0.0",
    summary: "A shopping or to-do list you both edit live from a conversation.",
    size: 30_000,
    carried: false,
    kind: "tool",
    locales: { es: { name: "Listas", summary: "Una lista de la compra o de tareas que editáis los dos." } },
  },
  { id: "com.flickertalk.sketch", name: "Sketch", version: "1.0.0", summary: "Draw with a finger.", size: 4_000, carried: true, kind: "tool", locales: { es: { name: "Dibujo" } } },
  { id: "com.flickertalk.game.tictactoe", name: "Tic-tac-toe", version: "1.0.0", summary: "Tic-tac-toe.", size: 50_000, carried: false, kind: "game", locales: { es: { name: "Tres en raya" } } },
];

test.use({ locale: "es-ES" });

test.beforeEach(async ({ app }) => {
  await app.addInitScript((entries) => {
    (window as unknown as Record<string, unknown>).__ftFakeCatalogue = entries;
  }, CATALOGUE);
});

test("Settings names an offered tool and an installed one in Spanish", async ({ app }) => {
  await app.goto("/plugins");
  const page = app.locator("ion-content.ft-plugins");
  // Offered: its name and summary from the catalogue.
  await expect(page).toContainText("Listas");
  await expect(page).toContainText("Una lista de la compra o de tareas que editáis los dos.");
  await expect(page).not.toContainText("A shopping or to-do list");
  // Installed: its package has no translation; the catalogue's entry with the same id has.
  await expect(page).toContainText("Dibujo");
  await expect(page).not.toContainText("Sketch");
});

test("the Games tab and an invitation's Play button name games in Spanish", async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as Record<string, unknown>).__ftFakeBobSays = ["🎮 Chess · Shall we play? https://flickertalk.com/games/chess"];
  });
  await app.goto("/tabs/games");
  await expect(app.getByTestId("my-games")).toContainText("Tres en raya");
  await expect(app.getByTestId("more-games")).toContainText("Ajedrez");
  await expect(app.getByTestId("more-games")).toContainText("Ajedrez para dos, jugada a jugada.");
  await expect(app.getByTestId(`install-${CHESS}`)).toBeVisible();

  await app.goto(`/chat/${BOB}`);
  const play = app.getByTestId("play-game").last();
  await expect(play).toContainText("Ajedrez");
  await expect(play).not.toContainText("Chess");
});
