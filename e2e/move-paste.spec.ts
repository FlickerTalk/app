import { readdirSync } from "node:fs";
import { expect, test } from "./helpers";

const LOCALES = readdirSync(new URL("../src/i18n", import.meta.url))
  .filter((file) => file.endsWith(".json"))
  .map((file) => file.replace(/\.json$/, ""));

const WIDTHS = [360, 384];

for (const mode of ["md", "ios"] as const) {
  for (const locale of LOCALES) {
    test.describe(`${mode}, ${locale}`, () => {
      test.use({ locale });

      test(`the paste hint fits its field at ${WIDTHS.join(" or ")} px`, async ({ app }) => {
        await app.goto(`/move?ionic:mode=${mode}`);
        const field = app.getByTestId("move-link");
        await expect(field).toBeVisible();
        await app.evaluate(() => document.fonts.ready);
        for (const width of WIDTHS) {
          await app.setViewportSize({ width, height: 800 });
          await app.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(done))));
          const fit = await field.evaluate((input: HTMLInputElement) => {
            const style = getComputedStyle(input);
            const context = document.createElement("canvas").getContext("2d")!;
            context.font = `${style.fontStyle} ${style.fontWeight} ${style.fontSize} ${style.fontFamily}`;
            const room = input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
            return { hint: input.placeholder, text: Math.ceil(context.measureText(input.placeholder).width), room: Math.floor(room) };
          });
          expect(fit.text, `"${fit.hint}" at ${width} px`).toBeLessThanOrEqual(fit.room);
        }
      });
    });
  }
}
