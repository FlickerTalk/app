// Ioan, 2026-10-09: a tool opened on its own (from the Apps tab) has no chat behind it. What it
// proposes to send asks who it is for, in the same sheet the games use; the contact picked gets
// their conversation with the proposal waiting in the composer. The user sends it, never the tool.
import type { Page } from "@playwright/test";
import { callsTo, expect, frameSays, test } from "./helpers";

const MARKDOWN = "com.flickertalk.markdown";
const BOB = "ft_bob123456789";

async function openFromApps(app: Page) {
  await app.goto("/tabs/apps");
  await app.getByTestId(`app-${MARKDOWN}`).click();
  await expect(app).toHaveURL(new RegExp(`/plugin/${MARKDOWN}$`));
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
}

/** The conversation's page on screen (`ion-app` is an `.ion-page` too). */
const chatPage = (app: Page) => app.locator("div.ion-page.ft-thread:not(.ion-page-hidden)");

test.describe("on a phone", () => {
  test.use({ viewport: { width: 360, height: 740 } });

  test("a text proposed by a tool on its own lands in the composer of the contact picked", async ({ app }) => {
    await openFromApps(app);
    await frameSays(app, { type: "ft.text", text: "# Title" });

    const picker = app.getByTestId("contact-picker");
    await expect(picker).toBeVisible();
    await expect(app.locator("ion-modal.ft-contact-picker.ft-sheet--window ion-title")).toHaveText("Send to");
    await expect(picker).toContainText("Bob");
    await app.getByTestId(`send-to-${BOB}`).click();

    await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
    await expect(chatPage(app).locator("ion-textarea textarea")).toHaveValue("# Title");
    await expect(app.getByTestId("contact-picker")).toBeHidden();
    // The tool's window went with its page; nothing went out.
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
    expect((await callsTo(app)).some(([command]) => command === "core_send")).toBe(false);
  });

  test("a file made by a tool on its own is staged in the chat picked, and sent only by the user", async ({ app }) => {
    await openFromApps(app);
    await frameSays(app, { type: "ft.made", name: "notes.md", mime: "text/markdown", data: "IyBoaQ==" });
    await app.getByTestId(`send-to-${BOB}`).click();

    await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
    const staged = chatPage(app).getByTestId("staged");
    await expect(staged).toContainText("notes.md");
    let calls = await callsTo(app);
    expect(calls.find(([command]) => command === "core_plugin_made")?.[1]).toMatchObject({ plugin: MARKDOWN, contact: BOB });
    expect(calls.some(([command]) => command === "core_send_picked")).toBe(false);

    await chatPage(app).getByTestId("staged-send").click();
    await expect(chatPage(app).getByTestId("staged")).toHaveCount(0);
    calls = await callsTo(app);
    expect(calls.find(([command]) => command === "core_send_picked")?.[1]).toMatchObject({ contact: BOB, file: { name: "notes.md" } });
  });

  test("no contact picked: the tool stays, and nothing is written", async ({ app }) => {
    await openFromApps(app);
    await frameSays(app, { type: "ft.made", name: "notes.md", mime: "text/markdown", data: "IyBoaQ==" });
    await expect(app.getByTestId("contact-picker")).toBeVisible();
    await app.keyboard.press("Escape");
    await expect(app.getByTestId("contact-picker")).toBeHidden();
    await expect(app).toHaveURL(new RegExp(`/plugin/${MARKDOWN}$`));
    await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
    expect((await callsTo(app)).some(([command]) => command === "core_plugin_made")).toBe(false);
  });
});

// A tablet (2026-10-02 sheets): the picker covers the whole window over the tool's page, and the
// conversation picked gets the proposal as on a phone.
test.describe("on a tablet", () => {
  test.use({ viewport: { width: 1280, height: 800 } });

  test("the picker covers the window, and the chat picked gets the text", async ({ app }) => {
    await openFromApps(app);
    await frameSays(app, { type: "ft.text", text: "# Title" });
    const sheet = app.locator("ion-modal.ft-contact-picker.ft-sheet--window .modal-wrapper");
    await expect(app.getByTestId(`send-to-${BOB}`)).toBeVisible();
    await expect.poll(async () => (await sheet.boundingBox())?.width).toBe(1280);
    await app.getByTestId(`send-to-${BOB}`).click();
    await expect(app).toHaveURL(new RegExp(`/chat/${BOB}$`));
    await expect(chatPage(app).locator("ion-textarea textarea")).toHaveValue("# Title");
  });
});
