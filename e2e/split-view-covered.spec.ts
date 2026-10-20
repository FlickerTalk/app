// app#197: on a tablet the conversation beside the list lives inside the Chats tab. A page pushed
// over the tabs (the contact page) covers it, and Ionic says so only to the tabs page, not to the
// tab inside it: a message arriving meanwhile must stay unread until the split view is seen again,
// as on a phone.
import { callsTo, expect, test } from "./helpers";

const BOB = "ft_bob123456789";

test.use({ viewport: { width: 1280, height: 800 } });

test("a message arriving under the contact page is read only once the split view is back", async ({ app }) => {
  await app.goto("/tabs/chats");
  const pane = app.locator(".ft-chats__detail");
  await expect(pane.getByText("see you at six")).toBeVisible();

  await app.locator("li").filter({ hasText: "Bob" }).getByTestId("chat-more").click();
  await expect(app).toHaveURL(new RegExp(`/contact/${BOB}$`));
  await expect(app.locator("[data-test='quick-actions']:visible")).toHaveCount(1);
  // The contact page is in: the tabs, with the split view, are hidden under it.
  await expect(pane).toBeHidden();

  const before = (await callsTo(app)).length;
  const since = async () => (await callsTo(app)).slice(before).map(([command]) => command);
  await app.evaluate((contact) => {
    const page = window as unknown as { __ftFakeBobSays: string[]; __ftFake: { emit: (event: string, payload: unknown) => void } };
    page.__ftFakeBobSays = ["are you there?"];
    page.__ftFake.emit("ft://changed", { contact });
  }, BOB);
  // The message reached the conversation under the contact page, and nobody has seen it.
  await expect(pane.getByText("are you there?")).toHaveCount(1);
  await app.waitForTimeout(500);
  expect(await since()).toContain("core_messages");
  expect(await since()).not.toContain("core_mark_read");

  // Back on the split view, the conversation is seen: now it is read.
  await app.locator("ion-back-button:visible").first().click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect(pane.getByText("are you there?")).toBeVisible();
  await expect.poll(since).toContain("core_mark_read");
});
