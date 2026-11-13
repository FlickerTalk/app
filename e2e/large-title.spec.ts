import { readdirSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

type Layout = {
  scrolled: number;
  scrollHeight: number;
  clientHeight: number;
  around: number;
  viewTop: number;
  barTop: number;
  barBottom: number;
  barHeight: number;
  smallTitle: boolean;
};

const SOURCES = fileURLToPath(new URL("../src/", import.meta.url));

const PAGES: Record<string, { path: string; rows: string[] }> = {
  "views/CallsPage.vue": { path: "/tabs/calls", rows: ["call-row"] },
  "views/ChatsPage.vue": { path: "/tabs/chats", rows: ["request-row", "circle-row", "chat-row"] },
  "views/SettingsPage.vue": { path: "/tabs/settings", rows: ["premium-badge"] },
};
const WIDTHS = [390, 1032];
const SHARES = [0.25, 0.5, 0.75];

const CALLS = Array.from({ length: 6 }, (_, at) => ({
  id: `call-${at}`,
  contact: "ft_bob123456789",
  name: "Bob",
  outgoing: at % 2 === 0,
  video: at % 3 === 0,
  startedAt: Date.now() - at * 3_600_000,
  seconds: at % 4 === 0 ? 0 : 75,
  outcome: at % 4 === 0 ? "missed" : "answered",
}));

const largeTitle = (app: Page) => app.locator(".ion-page:not(.ion-page-hidden) ion-title.title-large").last();

function layout(app: Page): Promise<Layout> {
  return largeTitle(app).evaluate(async (title) => {
    const content = title.closest("ion-content") as HTMLElement & { getScrollElement(): Promise<HTMLElement> };
    const scroller = await content.getScrollElement();
    const bar = title.closest("ion-toolbar")!.getBoundingClientRect();
    const main = content.parentElement!.querySelector(":scope > ion-header")!;
    return {
      scrolled: scroller.scrollTop,
      scrollHeight: scroller.scrollHeight,
      clientHeight: scroller.clientHeight,
      around: innerHeight - scroller.clientHeight,
      viewTop: scroller.getBoundingClientRect().top,
      barTop: bar.top,
      barBottom: bar.bottom,
      barHeight: bar.height,
      smallTitle: !main.classList.contains("header-collapse-condense-inactive"),
    };
  });
}

function scrollTo(app: Page, end: "top" | "bottom"): Promise<void> {
  return largeTitle(app).evaluate(async (title, to) => {
    const content = title.closest("ion-content") as HTMLElement & { scrollToTop(ms: number): Promise<void>; scrollToBottom(ms: number): Promise<void> };
    await content.scrollToTop(0);
    if (to === "bottom") await content.scrollToBottom(0);
  }, end);
}

async function settled(app: Page): Promise<Layout> {
  let last = await layout(app);
  for (let calm = 0; calm < 3; ) {
    await app.evaluate(() => new Promise((done) => requestAnimationFrame(() => setTimeout(done, 50))));
    const next = await layout(app);
    calm = next.scrollHeight === last.scrollHeight && next.clientHeight === last.clientHeight ? calm + 1 : 0;
    last = next;
  }
  return last;
}

async function resize(app: Page, width: number, height: number): Promise<Layout> {
  await app.setViewportSize({ width, height });
  await app.waitForFunction((tall) => innerHeight === tall, height);
  return settled(app);
}

async function open(app: Page, path: string, rows: string[], mode: "ios" | "md"): Promise<void> {
  await app.addInitScript((calls) => Object.assign(window, { __ftFakeCalls: calls }), CALLS);
  await app.goto(`${path}?ionic:mode=${mode}`);
  for (const row of rows) await expect(app.getByTestId(row).first()).toBeVisible();
}

async function longerThanTheScreen(app: Page, width: number): Promise<Layout> {
  const first = await resize(app, width, 600);
  const short = await resize(app, width, first.around + 100);
  expect(short.scrollHeight, "the page is longer than a short screen, so its length is its content's").toBeGreaterThan(short.clientHeight + 60);
  return short;
}

const whole = (one: Layout) => one.barTop >= one.viewTop - 0.5;
const away = (one: Layout) => one.barBottom <= one.viewTop + 0.5 && one.smallTitle;

async function settlesWholeOrAway(app: Page, message: string): Promise<Layout> {
  let last: Layout | undefined;
  await expect
    .poll(
      async () => {
        last = await layout(app);
        return whole(last) || away(last);
      },
      { message, timeout: 3000 },
    )
    .toBe(true);
  return last!;
}

test("checks every page that has a large title", () => {
  const found = (readdirSync(SOURCES, { recursive: true }) as string[])
    .filter((file) => file.endsWith(".vue") && readFileSync(`${SOURCES}${file}`, "utf8").includes('collapse="condense"'))
    .sort();
  expect(found).toEqual(Object.keys(PAGES).sort());
});

for (const [view, { path, rows }] of Object.entries(PAGES)) {
  const name = view.replace(/^views\/(.+)Page\.vue$/, "$1");

  for (const width of WIDTHS) {
    test(`${name}' large title never rests half under the top bar at ${width} px wide, however little the page scrolls (iOS)`, async ({ app }) => {
      await open(app, path, rows, "ios");
      await expect(largeTitle(app)).toBeVisible();
      const long = await longerThanTheScreen(app, width);

      for (const share of SHARES) {
        const overflow = Math.round(long.barHeight * share);
        await resize(app, width, long.scrollHeight - overflow + long.around);
        await scrollTo(app, "bottom");
        const end = await settlesWholeOrAway(app, `overflowing by ${overflow} px, scrolled to the end, the title is whole or gone into the bar`);
        expect(end.scrolled, "the page was scrolled").toBeGreaterThan(0);
      }

      await resize(app, width, long.scrollHeight + long.barHeight + long.around);
      await scrollTo(app, "top");
      await expect
        .poll(async () => {
          const atRest = await layout(app);
          return whole(atRest) && !atRest.smallTitle;
        }, { message: "a page that fits shows its large title whole and no small one", timeout: 3000 })
        .toBe(true);
      await scrollTo(app, "bottom");
      await settlesWholeOrAway(app, "a page that fits, scrolled to the end, has its title whole or gone into the bar");
    });

    test(`${name} scrolls exactly as far as its content on Android at ${width} px wide`, async ({ app }) => {
      await open(app, path, rows, "md");
      await expect(largeTitle(app)).toBeAttached();
      const long = await longerThanTheScreen(app, width);

      for (const overflow of [-24, 12, 24, 36]) {
        const now = await resize(app, width, long.scrollHeight - overflow + long.around);
        expect(now.scrollHeight - now.clientHeight, `overflowing by ${overflow} px`).toBe(Math.max(0, overflow));
      }
    });
  }
}
