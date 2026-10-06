// The contact page's quick actions (Ioan, 2026-10-06): as in Messenger and WhatsApp, the chat
// header keeps three icons and the search of the conversation starts from the contact page.
import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";

test("the contact page's search opens the conversation with its search, once", async ({ app }) => {
  await app.goto(`/chat/${BOB}`);
  await app.locator(".ft-thread__bar [data-test='peer']:visible").click();
  await expect(app).toHaveURL(new RegExp(`/contact/${BOB}$`));
  const row = app.locator("[data-test='quick-actions']:visible");
  await expect(row.locator("[data-test^='quick-']")).toHaveCount(3);
  await expect(row.getByTestId("quick-call")).toBeEnabled();
  await expect(row.getByTestId("quick-video")).toBeEnabled();

  await row.getByTestId("quick-search").click();
  await expect(app.locator("[data-test='search-panel']:visible")).toHaveCount(1);
  // The address forgets the search, so coming back to the conversation does not open it again.
  await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
});
