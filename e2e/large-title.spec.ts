import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const WIDTH = 1032;

type Layout = {
  scrolled: number;
  scrollHeight: number;
  around: number;
  viewTop: number;
  barTop: number;
  barBottom: number;
  barHeight: number;
  smallTitle: boolean;
};

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
      around: innerHeight - scroller.clientHeight,
      viewTop: scroller.getBoundingClientRect().top,
      barTop: bar.top,
      barBottom: bar.bottom,
      barHeight: bar.height,
      smallTitle: !main.classList.contains("header-collapse-condense-inactive"),
    };
  });
}

function scrollToEnd(app: Page): Promise<void> {
  return largeTitle(app).evaluate(async (title) => {
    const content = title.closest("ion-content") as HTMLElement & { scrollToTop(ms: number): Promise<void>; scrollToBottom(ms: number): Promise<void> };
    await content.scrollToTop(0);
    await content.scrollToBottom(0);
  });
}

const whole = (one: Layout) => one.barTop >= one.viewTop - 0.5;
const away = (one: Layout) => one.barBottom <= one.viewTop + 0.5 && one.smallTitle;

test.use({ viewport: { width: WIDTH, height: 600 } });

test("Settings' large title never rests half under the top bar, however little the page scrolls (iOS)", async ({ app }) => {
  await app.goto("/tabs/settings?ionic:mode=ios");
  await expect(largeTitle(app)).toBeVisible();
  const long = await layout(app);
  for (const share of [0.25, 0.5, 0.75]) {
    const overflow = Math.round(long.barHeight * share);
    await app.setViewportSize({ width: WIDTH, height: long.scrollHeight - overflow + long.around });
    await scrollToEnd(app);
    let last: Layout | undefined;
    await expect
      .poll(
        async () => {
          last = await layout(app);
          return whole(last) || away(last);
        },
        { message: `overflowing by ${overflow} px, scrolled to the end, the title is whole or gone into the bar`, timeout: 3000 },
      )
      .toBe(true);
    expect(last!.scrolled, "the page was scrolled").toBeGreaterThan(0);
  }
});
