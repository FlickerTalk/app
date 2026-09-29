// A5 of the 2026-09-24 review: whoever wrote first with this phone's link waits in the requests,
// apart from the list, until the user says yes or no. Since 2026-09-28 the yes and the no are in
// the conversation, as in WhatsApp, not on the row: a small screen made a wrong tap too easy.
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

  await requests.getByTestId("request-row").click();
  await app.getByTestId("request-accept").click();
  await expect(app.getByTestId("request-panel")).toHaveCount(0);
  expect(await commandsSent(app)).toContain("core_accept_contact");

  // Back to the list without reloading: a reload would bring back the fake core's first state.
  await app.goBack();
  await expect(app.getByTestId("requests")).toHaveCount(0);
  await expect(app.locator("[data-test='chat-row']").filter({ visible: true })).toHaveCount(2);
});

test("a no asks once, blocks the stranger and takes them out of the requests", async ({ app }) => {
  await app.goto("/tabs/chats");
  await app.getByTestId("requests").getByTestId("request-row").click();
  await app.getByTestId("request-decline").click();
  expect(await commandsSent(app)).not.toContain("core_decline_contact");

  await app.getByTestId("request-decline-confirm").click();
  await expect(app.getByTestId("requests")).toHaveCount(0);
  await expect(app.locator("[data-test='chat-row']").filter({ visible: true })).toHaveCount(1);
  expect(await commandsSent(app)).toContain("core_decline_contact");
});
