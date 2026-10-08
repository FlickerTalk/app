// Links that open the app (2026-10-08, App Links and Universal Links): the phone hands the app a
// `https://flickertalk.com/add#…` or `/move#…` link. A link never acts alone: it opens its page
// with the field filled, and nothing is added or moved until the user taps.
import { callsTo, commandsSent, expect, test } from "./helpers";

const CARD = "https://flickertalk.com/add#oWR2ZXJzaW9uAWRuYW1lY0FuYQ";
const INVITE = "https://flickertalk.com/move#omRjYXJkWCCnZXNlY3JldFgg";

/** The phone opens the app with `link` (an init script, with the link as its argument). */
function opensWith(link: string) {
  (window as unknown as Record<string, unknown>).__ftFakeOpenedLink = link;
}

test("a contact link that opened the app fills Add contact, and adds only on Add", async ({ app }) => {
  await app.addInitScript(opensWith, CARD);
  await app.goto("/tabs/chats");
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
  await expect(app.getByRole("alert")).toHaveCount(0);
  expect(await commandsSent(app)).not.toContain("core_add_contact");

  await app.getByTestId("add").click();
  await expect.poll(async () => (await callsTo(app)).find(([command]) => command === "core_add_contact")?.[1]).toEqual({ link: CARD });
});

test("a broken link opens Add contact and says it is not a contact link", async ({ app }) => {
  const cut = "https://flickertalk.com/add#oWR2";
  await app.addInitScript(opensWith, cut);
  await app.goto("/tabs/chats");
  await expect(app.getByTestId("paste")).toHaveValue(cut);
  await expect(app.getByRole("alert")).toHaveText("That is not a FlickerTalk contact link");
});

test("an invite that opened the app fills the old phone's move screen, and moves only on asking", async ({ app }) => {
  await app.addInitScript(opensWith, INVITE);
  await app.goto("/tabs/chats");
  await expect(app).toHaveURL(/\/move$/);
  await expect(app.getByTestId("move-link")).toHaveValue(INVITE);
  expect(await commandsSent(app)).not.toContain("core_move_to");
});

// The usual case: the link was sent, the app installed, and the link tapped again.
test("on the first run the link waits for Start", async ({ app }) => {
  await app.addInitScript(opensWith, CARD);
  await app.addInitScript(() => localStorage.removeItem("ft-onboarded"));
  await app.goto("/");
  await expect(app).toHaveURL(/\/welcome$/);
  await app.getByTestId("start").click();
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
  expect(await commandsSent(app)).not.toContain("core_add_contact");
});

test("a link that opens the app while it runs goes to Add contact from wherever it is", async ({ app }) => {
  await app.goto("/chat/ft_bob123456789");
  await expect(app.locator(".ft-bubble").first()).toBeVisible();
  await app.evaluate((link) => (window as unknown as { __ftFake: { openLink: (url: string) => void } }).__ftFake.openLink(link), CARD);
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
});

// Inside a chat a FlickerTalk link is the app's own: it never goes out to the system's browser.
test("a contact link tapped in a chat opens Add contact in the app", async ({ app }) => {
  await app.addInitScript((link) => {
    (window as unknown as Record<string, unknown>).__ftFakeBobSays = [`Add Ana: ${link}`];
  }, CARD);
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("link").filter({ hasText: CARD }).click();
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
  const sent = await commandsSent(app);
  expect(sent).toContain("core_read_link");
  expect(sent).not.toContain("plugin:opener|open_url");
  expect(sent).not.toContain("core_add_contact");
});

// What is open on top closes first, as with Back: here the camera scanner over Add contact.
test("a link that opens the app closes the scanner first", async ({ app }) => {
  await app.goto("/add-contact");
  await app.getByTestId("mode-scan").click();
  await app.getByTestId("scan-now").click();
  await expect(app.getByTestId("scanner-overlay")).toBeVisible();
  await app.evaluate((link) => (window as unknown as { __ftFake: { openLink: (url: string) => void } }).__ftFake.openLink(link), CARD);
  await expect(app.getByTestId("scanner-overlay")).toBeHidden();
  expect(await commandsSent(app)).toContain("plugin:barcode-scanner|cancel");
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
  expect(await commandsSent(app)).not.toContain("core_add_contact");
  expect(await app.evaluate(() => document.documentElement.classList.contains("ft-scanning"))).toBe(false);
});
