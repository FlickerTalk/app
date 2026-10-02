// A suggestion from Settings (2026-10-02), on a phone in Arabic: the page reads right to left, the
// counter keeps its digits in order, and a send the core does not confirm (here the fake core has
// no such command, like an app talking to a router without the endpoint) never says "sent" and
// keeps what was written.
import { commandsSent, expect, test } from "./helpers";

test.describe("in Arabic, right to left", () => {
  test.use({ viewport: { width: 360, height: 740 }, locale: "ar" });

  test("a suggestion that does not go keeps its text", async ({ app }) => {
    await app.goto("/feedback");
    // The app sets the direction once the language is loaded: wait for it, never read it once.
    await expect(app.locator("html")).toHaveAttribute("dir", "rtl");
    await expect(app.getByTestId("hint")).toHaveText("يصلنا اقتراحك دون اسمك أو أي معرّف. لا يمكننا الرد عليك. لا تكتب بيانات شخصية.");
    const counter = app.getByTestId("counter");
    await expect(counter).toHaveText("0 / 2000");
    await expect(counter).toHaveCSS("direction", "ltr");
    await expect(app.getByTestId("send")).toHaveAttribute("disabled", "");

    await app.locator("ion-textarea textarea").fill("ملصقات من فضلكم");
    await expect(counter).toHaveText("15 / 2000");
    await app.getByTestId("send").click();
    await expect(app.getByTestId("outcome")).toHaveText("تعذّر الإرسال. حاول لاحقًا");
    await expect(app.locator("ion-textarea textarea")).toHaveValue("ملصقات من فضلكم");
    await expect.poll(() => commandsSent(app)).toContain("core_send_feedback");
  });
});
