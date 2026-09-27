// A5 of the 2026-09-24 review: whoever wrote first with this phone's link waits in the requests,
// apart from the list, until the user says yes or no.
import { commandsSent, expect, test } from "./helpers";

test("a stranger waits in the requests and joins the list on a yes", async ({ app }) => {
  await app.goto("/tabs/chats");
  const requests = app.getByTestId("requests");
  await expect(requests).toBeVisible();
  await expect(requests).toContainText("Mamá");
  // The short id next to the name: a "Mamá" with a new number is a stranger until proven.
  await expect(requests).toContainText("ft_strang");
  await expect(requests).toContainText("hey, it's me, new number");
  // Not among the conversations.
  await expect(app.locator("[data-test='chat-row']")).toHaveCount(1);

  await requests.getByRole("button", { name: "Accept" }).click();
  await expect(app.getByTestId("requests")).toHaveCount(0);
  await expect(app.locator("[data-test='chat-row']")).toHaveCount(2);
  expect(await commandsSent(app)).toContain("core_accept_contact");
});

test("a no blocks the stranger and takes them out of the requests", async ({ app }) => {
  await app.goto("/tabs/chats");
  await app.getByTestId("requests").getByRole("button", { name: "Decline and block" }).click();
  await expect(app.getByTestId("requests")).toHaveCount(0);
  await expect(app.locator("[data-test='chat-row']")).toHaveCount(1);
  expect(await commandsSent(app)).toContain("core_decline_contact");
});
