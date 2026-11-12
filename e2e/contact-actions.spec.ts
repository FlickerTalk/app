import { expect, test } from "./helpers";

const BOB = "ft_bob123456789";

test("the contact page's search goes back to the conversation and searches there", async ({ app }) => {
  await app.goto("/tabs/chats");
  await app.getByTestId("chat-row").filter({ hasText: "Bob" }).first().click();
  await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
  await app.locator(".ft-thread__bar [data-test='peer']:visible").click();
  await expect(app).toHaveURL(new RegExp(`/contact/${BOB}$`));
  const row = app.locator("[data-test='quick-actions']:visible");
  await expect(row.locator("[data-test^='quick-']")).toHaveCount(3);
  await expect(row.getByTestId("quick-call")).toBeEnabled();
  await expect(row.getByTestId("quick-video")).toBeEnabled();

  await row.getByTestId("quick-search").click();
  await expect(app.locator("[data-test='search-panel']:visible")).toHaveCount(1);
  await expect(app.locator("[data-test='search-panel']:visible ion-searchbar input")).toBeFocused();
  await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
  await app.locator("ion-back-button:visible").first().click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
});

test("reached from elsewhere, the contact page's search opens the conversation, once", async ({ app }) => {
  await app.goto(`/contact/${BOB}`);
  await app.locator("[data-test='quick-search']:visible").click();
  await expect(app.locator("[data-test='search-panel']:visible")).toHaveCount(1);
  await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
});
