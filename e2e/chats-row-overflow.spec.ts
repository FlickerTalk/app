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
    expect(Math.abs(boxes[0].x - boxes[1].x)).toBeLessThanOrEqual(1);

    const cut = await app
      .getByTestId("chat-row")
      .filter({ hasText: "Bob" })
      .locator(".ft-row__preview")
      .evaluate((el) => ({ truncates: el.scrollWidth > el.clientWidth, ellipsis: getComputedStyle(el).textOverflow }));
    expect(cut).toEqual({ truncates: true, ellipsis: "ellipsis" });
  });
});
