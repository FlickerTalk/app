// Circles (2026-09-27): a small closed group of contacts. The list shows it with who said the
// last thing; a new one takes a name and some contacts; inside, who said what is over each
// bubble; its settings let an admin add and remove people, and anyone leave.
import { callsTo, commandsSent, expect, test } from "./helpers";

test("the list shows the circle and who said the last thing", async ({ app }) => {
  await app.goto("/tabs/chats");
  const row = app.getByTestId("circle-row");
  await expect(row).toHaveCount(1);
  await expect(row).toContainText("Friends");
  await expect(row).toContainText("Bob: dinner on friday?");
  await expect(row.getByTestId("unread")).toHaveText("1");
});

test("a new circle takes a name and some contacts, and opens", async ({ app }) => {
  await app.goto("/tabs/chats");
  // The stranger in the requests is not offered: only contacts are.
  await app.getByTestId("new-circle").click();
  await expect(app).toHaveURL(/\/new-circle$/);
  await expect(app.getByRole("checkbox")).toHaveCount(1);
  const create = app.getByTestId("circle-create");
  await expect(create).toBeDisabled();
  await app.getByTestId("circle-name").fill("Poker night");
  await app.getByTestId("pick-ft_bob123456789").click();
  await create.click();
  await expect(app).toHaveURL(/\/circle\/circle2$/);
  await expect(app.getByTestId("circle-event")).toContainText("You created the circle «Poker night»");
  const made = (await callsTo(app)).find(([command]) => command === "core_circle_create");
  expect(made?.[1]).toMatchObject({ name: "Poker night", members: ["ft_bob123456789"] });
});

test("inside, who said what is over each bubble, and a text goes to the circle", async ({ app }) => {
  await app.goto("/circle/circle1");
  await expect(app.getByTestId("sender")).toHaveText("Bob");
  await expect(app.getByTestId("circle-event")).toHaveText("You created the circle «Friends»");
  await expect.poll(async () => (await commandsSent(app)).includes("core_circle_mark_read")).toBe(true);
  await app.locator("ion-textarea textarea").fill("count me in");
  await app.getByTestId("circle-send").click();
  await expect.poll(async () => (await callsTo(app)).some(([command, args]) => command === "core_circle_send" && args?.text === "count me in")).toBe(true);
  // The core announced the change: the new bubble shows without leaving the screen.
  await expect(app.getByTestId("bubble").last()).toContainText("count me in");
});

test("an admin adds and removes people; removing asks once", async ({ app }) => {
  await app.goto("/circle/circle1/info");
  await expect(app.getByTestId("circle-member")).toHaveCount(2);
  await expect(app.getByTestId("circle-admin-badge")).toHaveCount(1);
  // Everyone of the list is in already: nobody to add. Accepting the stranger makes someone.
  await app.getByTestId("circle-invite").click();
  await expect(app.getByTestId("circle-candidates")).toContainText("Every contact of yours is in it already");

  await app.getByTestId("circle-remove").click();
  expect(await commandsSent(app)).not.toContain("core_circle_remove");
  await app.getByTestId("circle-remove-sure").click();
  await expect.poll(async () => (await commandsSent(app)).includes("core_circle_remove")).toBe(true);
  await expect(app.getByTestId("circle-member")).toHaveCount(1);
});

test("only admins write when the switch is on, and leaving asks once", async ({ app }) => {
  await app.goto("/circle/circle1/info");
  await app.getByTestId("circle-admins-only").click();
  await expect.poll(async () => (await callsTo(app)).some(([command, args]) => command === "core_circle_admins_only" && args?.adminsOnly === true)).toBe(true);

  await app.getByTestId("circle-leave").click();
  expect(await commandsSent(app)).not.toContain("core_circle_leave");
  await app.getByTestId("circle-leave-sure").click();
  await expect(app.getByTestId("circle-left")).toBeVisible();
  await expect(app.getByTestId("circle-forget")).toBeVisible();
  // Back to the conversation without reloading: the fake core would start over.
  await app.getByLabel("Back").click();
  await expect(app.getByTestId("circle-left")).toHaveText("You are no longer in this circle");
  await expect(app.getByTestId("circle-send")).toHaveCount(0);
});
