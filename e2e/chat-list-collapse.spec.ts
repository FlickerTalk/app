import type { Locator, Page } from "@playwright/test";
import { expect, test } from "./helpers";

type Box = { x: number; y: number; width: number; height: number };

const APPEARANCE = process.env.FT_APPEARANCE ?? "dark";

async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(450);
  await app.screenshot({ path: `${dir}/${name}-${APPEARANCE}.png` });
}

async function settled(target: Locator): Promise<Box> {
  let last = "";
  await expect
    .poll(async () => {
      const now = JSON.stringify(await target.boundingBox());
      const still = now === last && now !== "null";
      last = now;
      return still;
    }, { intervals: [100] })
    .toBe(true);
  return (await target.boundingBox())!;
}

const box = async (app: Page, selector: string) => (await app.locator(selector).boundingBox())!;
const near = (actual: number, expected: number, what: string) => expect(Math.abs(actual - expected), `${what}: ${actual}, not ${expected}`).toBeLessThanOrEqual(1);
const stripWidth = (app: Page) => app.evaluate(() => parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--ft-list-width")));

function collapsedBefore(app: Page) {
  return app.addInitScript(() => localStorage.setItem("ft-chat-list", "collapsed"));
}

async function chatsWithSession(app: Page) {
  await app.goto("/tabs/chats");
  await app.evaluate(() => {
    const last = { id: "pablo-last", outgoing: false, text: "in the session", sentAt: Date.now(), state: "delivered" };
    const pablo = { id: "ft_pablo12345678", name: "Pablo", unread: 2, blocked: false, connected: false, last };
    localStorage.setItem("ft-fake-sessions", JSON.stringify({ sessions: { s1: { id: "s1", pin: "111111" } }, open: ["s1"], next: 2, chats: { s1: [pablo] } }));
  });
  await app.reload();
  await expect(app.getByTestId("session-section")).toHaveCount(1);
}

function withNothing(app: Page) {
  return app.addInitScript(() => {
    const fake = (window as unknown as { __TAURI_INTERNALS__: { invoke: (command: string, args?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
    const invoke = fake.invoke;
    fake.invoke = (command, args) => (["core_conversations", "core_requests", "core_circles"].includes(command) ? invoke(command, args).then(() => []) : invoke(command, args));
  });
}

const HIDDEN_ACTIONS = /^(header-actions|chat-more|circle-more|session-add|session-new-circle|session-remove|session-close)$/;

async function tabbedFrom(app: Page, start: Locator): Promise<string[]> {
  await start.focus();
  const reached: string[] = [];
  for (let step = 0; step < 12; step++) {
    await app.keyboard.press("Tab");
    reached.push(
      await app.evaluate(() => {
        const focused = document.activeElement;
        if (!focused) return "";
        if (focused.closest('.ft-chats__head ion-buttons[slot="end"]')) return "header-actions";
        return focused.closest("[data-test]")?.getAttribute("data-test") ?? focused.tagName.toLowerCase();
      }),
    );
  }
  expect(reached, "the keyboard reaches the rows").toContain("chat-row");
  return reached;
}

test.beforeEach(async ({ app }) => {
  await app.addInitScript((chosen) => localStorage.setItem("ft-appearance", chosen), APPEARANCE);
});

const TABLETS = [
  { name: "landscape", width: 1024, height: 768 },
  { name: "portrait", width: 768, height: 1024 },
];

for (const tablet of TABLETS) {
  for (const locale of ["en", "ar"]) {
    test.describe(`${tablet.name}, ${locale}`, () => {
      test.use({ viewport: { width: tablet.width, height: tablet.height }, locale });

      test("the list collapses to a strip of names and the chat pane takes the room, its sheets included", async ({ app }) => {
        await chatsWithSession(app);
        const toggle = app.getByTestId("list-toggle");
        await expect(toggle).toHaveAttribute("aria-expanded", "true");
        const expanded = await settled(app.locator(".ft-chats__list"));
        await shot(app, `${tablet.name}-${locale}-expanded`);

        await expect(toggle).toHaveAttribute("aria-controls", "ft-chat-list");
        await expect(app.locator("#ft-chat-list")).toHaveCount(1);
        if (locale === "en") await expect(toggle).toHaveAccessibleName("Collapse chat list");
        await toggle.click();
        await expect(toggle).toHaveAttribute("aria-expanded", "false");
        await expect(toggle).toBeFocused();
        if (locale === "en") await expect(toggle).toHaveAccessibleName("Expand chat list");
        const strip = await stripWidth(app);
        expect(strip, "a strip, far narrower than the list").toBeLessThan(expanded.width / 2);
        const list = await settled(app.locator(".ft-chats__list"));
        near(list.width, strip, "the list is the strip");
        const head = await box(app, ".ft-chats__head");
        near(head.x, list.x, "the header starts with the list");
        near(head.width, strip, "the header is the strip");
        const pane = await box(app, ".ft-chats__detail");
        if (locale === "ar") {
          near(pane.x, 0, "the pane starts at the screen's edge");
          near(pane.x + pane.width, list.x, "the pane ends at the strip");
        } else {
          near(pane.x, list.x + list.width, "the pane starts at the strip");
          near(pane.x + pane.width, tablet.width, "the pane ends at the screen's edge");
        }

        await expect(app.locator(".ft-chats__head ion-title")).toBeHidden();
        await expect(app.getByTestId("new-circle")).toBeHidden();
        const bob = app.getByTestId("chat-row").filter({ hasText: "Bob" });
        await expect(bob.locator(".ft-row__name")).toBeVisible();
        await expect(bob.locator(".ft-row__preview")).toBeHidden();
        await expect(bob.locator(".ft-row__time")).toBeHidden();
        await expect(app.getByTestId("chat-more").first()).toBeHidden();
        await expect(app.getByTestId("circle-row").getByTestId("unread")).toBeVisible();
        await expect(app.getByTestId("request-row").locator(".ft-row__name")).toBeVisible();
        await expect(app.getByTestId("request-row").locator(".ft-row__time")).toBeHidden();
        await expect(app.locator(".ft-requests__hint")).toBeHidden();
        await expect(app.getByTestId("requests").locator(".ft-requests__mark")).toBeVisible();
        if (locale === "en") await expect(app.getByRole("region", { name: "Requests" })).toBeVisible();
        const session = app.getByTestId("session-section");
        await expect(session.getByTestId("session-toggle")).toBeVisible();
        await expect(session.getByTestId("chat-row").getByTestId("unread")).toBeVisible();
        for (const name of await app.locator(".ft-row__name:visible").all()) {
          const edges = (await name.boundingBox())!;
          expect(edges.x, "the name stays inside the strip").toBeGreaterThanOrEqual(list.x - 1);
          expect(edges.x + edges.width, "the name stays inside the strip").toBeLessThanOrEqual(list.x + list.width + 1);
        }
        expect((await tabbedFrom(app, toggle)).filter((reached) => HIDDEN_ACTIONS.test(reached))).toEqual([]);
        await shot(app, `${tablet.name}-${locale}-collapsed`);

        await app.locator(".ft-chats__detail [data-test='apps']").click();
        const sheet = await settled(app.locator("ion-modal.ft-apps-sheet .modal-wrapper"));
        near(sheet.x, pane.x, "the sheet starts with the pane");
        near(sheet.width, pane.width, "the sheet is as wide as the pane");
        await shot(app, `${tablet.name}-${locale}-collapsed-apps-sheet`);
        await app.keyboard.press("Escape");
        await expect(app.locator("ion-modal.ft-apps-sheet .modal-wrapper")).toBeHidden();

        await toggle.click();
        near((await settled(app.locator(".ft-chats__list"))).width, expanded.width, "the list is back to its width");
      });
    });
  }
}

test.describe("a tablet that was left with the strip", () => {
  test.use({ viewport: { width: 1024, height: 768 } });

  test("draws the strip from the first frame, never the wide list", async ({ app }) => {
    await collapsedBefore(app);
    await app.addInitScript(() => {
      const seen: number[] = [];
      (window as unknown as { __listWidths: number[] }).__listWidths = seen;
      const watch = () => {
        const list = document.querySelector(".ft-chats__list");
        if (list) seen.push(Math.round(list.getBoundingClientRect().width));
        requestAnimationFrame(watch);
      };
      requestAnimationFrame(watch);
    });
    await app.goto("/tabs/chats");
    await expect(app.getByTestId("list-toggle")).toHaveAttribute("aria-expanded", "false");
    const strip = await stripWidth(app);
    expect(strip, "a strip, not the wide list").toBeLessThan(180);
    const widths = await app.evaluate(() => (window as unknown as { __listWidths: number[] }).__listWidths);
    expect(widths.length).toBeGreaterThan(0);
    expect(new Set(widths)).toEqual(new Set([strip]));
  });

  test("with nothing in it, keeps only the round add-contact button", async ({ app }) => {
    await collapsedBefore(app);
    await withNothing(app);
    await app.goto("/tabs/chats");
    const empty = app.getByTestId("empty");
    await expect(empty).toBeVisible();
    await expect(empty).toHaveText("");
    await expect(empty.getByTestId("tour-link")).toHaveCount(0);
    const add = empty.getByRole("button", { name: "Add contact" });
    await expect(add).toBeVisible();
    const list = await box(app, ".ft-chats__list");
    const button = (await add.boundingBox())!;
    expect(button.x).toBeGreaterThanOrEqual(list.x);
    expect(button.x + button.width).toBeLessThanOrEqual(list.x + list.width);
    await shot(app, "landscape-en-collapsed-empty");
    await add.click();
    await expect(app).toHaveURL(/\/add-contact$/);
  });
});

test.describe("a phone", () => {
  test.use({ viewport: { width: 390, height: 844 } });

  test("keeps the whole list and offers no toggle, whatever a tablet chose", async ({ app }) => {
    await collapsedBefore(app);
    await app.goto("/tabs/chats");
    await expect(app.getByTestId("chat-row").filter({ hasText: "Bob" }).locator(".ft-row__preview")).toBeVisible();
    await expect(app.getByTestId("list-toggle")).toHaveCount(0);
    near((await box(app, ".ft-chats__list")).width, 390, "the list spans the phone");
  });
});
