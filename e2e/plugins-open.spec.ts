// Settings → Plugins (found 2026-10-02): "Open" shares the remove button's reset and was drawn in its
// danger red, so opening a plugin looked like removing it. It is the accent; remove stays red.
import { expect, test } from "./helpers";

const MARKDOWN = "com.flickertalk.markdown";

for (const [direction, appearance] of [["mono", "dark"], ["ember", "light"]]) {
  test(`${direction}, ${appearance}: open is the accent, remove is not`, async ({ app }) => {
    await app.addInitScript(
      ([chosenDirection, chosenAppearance]) => {
        localStorage.setItem("ft-direction", chosenDirection);
        localStorage.setItem("ft-appearance", chosenAppearance);
      },
      [direction, appearance],
    );
    await app.goto("/plugins");
    const open = app.getByTestId(`open-${MARKDOWN}`);
    const remove = app.getByTestId(`remove-${MARKDOWN}`);
    await expect(open).toBeVisible();
    // The accent as the browser resolves it, read off a probe painted with it.
    const accent = await app.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.color = "var(--ft-accent)";
      document.body.append(probe);
      const colour = getComputedStyle(probe).color;
      probe.remove();
      return colour;
    });
    const colour = (one: typeof open) => one.evaluate((node) => getComputedStyle(node).color);
    expect(await colour(open)).toBe(accent);
    expect(await colour(remove)).not.toBe(await colour(open));
  });
}
