import { expect, holdTile, test } from "./helpers";

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
    await app.goto("/tabs/apps");
    await holdTile(app, `app-${MARKDOWN}`);
    const open = app.getByTestId("sheet-open");
    const remove = app.getByTestId("sheet-remove");
    await expect(open).toBeVisible();
    const resolved = (colour: string) =>
      app.evaluate((css) => {
        const probe = document.createElement("span");
        probe.style.color = css;
        document.body.append(probe);
        const seen = getComputedStyle(probe).color;
        probe.remove();
        return seen;
      }, colour);
    const native = (one: typeof open, property: "backgroundColor" | "color") =>
      one.evaluate((node, which) => getComputedStyle(node.shadowRoot!.querySelector(".button-native")!)[which], property);
    expect(await native(open, "backgroundColor")).toBe(await resolved("var(--ion-color-primary)"));
    expect(await native(remove, "color")).toBe(await resolved("var(--ion-color-danger)"));
    expect(await native(remove, "color")).not.toBe(await native(open, "backgroundColor"));
  });
}
