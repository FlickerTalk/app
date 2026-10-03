// app#69 (2026-10-03, seen on a Samsung at 384 px and an iPhone at 375 px): a last message with no
// space to break on pushed the row's ⋮ (the contact's settings) past the edge of the screen. The
// preview must give way, cut with an ellipsis, and every ⋮ stay inside the screen, in line with
// the other rows.
import { expect, test } from "./helpers";

const LONG = "🎮 Shall we play Chess? https://flickertalk.com/games/chess/" + "x".repeat(80);

test.describe("on a 384 px phone", () => {
  test.use({ viewport: { width: 384, height: 800 } });

  test("a long preview keeps the row's ⋮ on screen and in line", async ({ app }) => {
    await app.addInitScript((long) => {
      const fake = window as unknown as Record<string, unknown>;
      fake.__ftFakeBobLast = long;
      fake.__ftFakeCarol = true;
    }, LONG);
    await app.goto("/tabs/chats");
    const more = app.getByTestId("chat-more");
    await expect(more).toHaveCount(2);

    const width = app.viewportSize()!.width;
    const boxes = await Promise.all((await more.all()).map(async (one) => (await one.boundingBox())!));
    for (const box of boxes) {
      expect(box.x + box.width, `the ⋮ ${JSON.stringify(box)} inside the screen`).toBeLessThanOrEqual(width);
    }
    // Bob's row (long preview) and Carol's (short) end at the same place.
    expect(Math.abs(boxes[0].x - boxes[1].x)).toBeLessThanOrEqual(1);

    const cut = await app
      .getByTestId("chat-row")
      .filter({ hasText: "Bob" })
      .locator(".ft-row__preview")
      .evaluate((el) => ({ truncates: el.scrollWidth > el.clientWidth, ellipsis: getComputedStyle(el).textOverflow }));
    expect(cut).toEqual({ truncates: true, ellipsis: "ellipsis" });
  });
});
