import type { Page } from "@playwright/test";
import { callsTo, expect, test } from "./helpers";

type Fake = { emit: (event: string, payload: unknown) => void };

async function lastLayout(page: Page) {
  const layouts = (await callsTo(page)).filter(([command]) => command === "core_call_video_layout");
  return layouts.at(-1)?.[1]?.layout;
}

async function theirCamera(page: Page, on: boolean, paused = false) {
  await page.evaluate(
    ([remote, remotePaused]) => {
      const fake = (window as unknown as { __ftFake: Fake & { state: { video: Record<string, unknown> } } }).__ftFake;
      Object.assign(fake.state.video, { remote, remotePaused });
      fake.emit("ft://call", { contact: "ft_bob123456789", call: "call-e2e", kind: "video", ...fake.state.video });
    },
    [on, paused],
  );
}

async function voiceCall(page: Page, button = "Voice call") {
  await page.addInitScript(() => {
    (window as unknown as { __ftFakeNative: boolean }).__ftFakeNative = true;
  });
  await page.goto("/chat/ft_bob123456789");
  await page.getByRole("button", { name: button, exact: true }).click();
  await expect(page.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
}

test("a voice call goes to video and back, from either side", async ({ app }) => {
  await voiceCall(app);
  expect((await callsTo(app)).find(([command]) => command === "core_call_start_native")?.[1]).toMatchObject({ video: false });
  const camera = app.getByRole("button", { name: "Camera", exact: true });
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(app.getByRole("button", { name: "Switch camera" })).toHaveAttribute("aria-disabled", "true");

  await camera.click();
  await expect(camera).toHaveAttribute("aria-pressed", "true");
  await expect(app.getByTestId("local-slot")).toBeVisible();
  await expect(app.locator("html")).toHaveClass(/ft-call-video/);
  await expect.poll(async () => (await lastLayout(app))?.local).not.toBeNull();
  await app.getByRole("button", { name: "Switch camera" }).click();
  await expect.poll(async () => (await lastLayout(app))?.mirrorLocal).toBe(false);

  await theirCamera(app, true);
  await expect(app.getByTestId("remote-slot")).toBeVisible();
  await expect(app.getByTestId("local-slot")).toHaveClass(/is-thumb/);
  await expect.poll(async () => (await lastLayout(app))?.remote).not.toBeNull();

  await theirCamera(app, true, true);
  await expect(app.getByTestId("remote-paused")).toContainText("Camera paused");

  await camera.click();
  await theirCamera(app, false);
  await expect(app.getByTestId("remote-slot")).toHaveCount(0);
  await expect(app.getByTestId("local-slot")).toHaveCount(0);
  await expect(app.locator("html")).not.toHaveClass(/ft-call-video/);
  await expect.poll(async () => lastLayout(app)).toMatchObject({ remote: null, local: null });

  await app.getByRole("button", { name: "Hang up" }).click();
  await expect.poll(async () => lastLayout(app)).toBeNull();
});

test("their camera coming on invites me to turn mine on", async ({ app }) => {
  await voiceCall(app);
  await theirCamera(app, true);
  await expect(app.getByText("Turn on your camera")).toBeVisible();
  await expect(app.getByRole("button", { name: "Camera", exact: true })).toHaveClass(/is-invite/);
});

test("a camera that is not allowed leaves a voice call and says how to allow it", async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as { __ftFakeCameraDenied: boolean }).__ftFakeCameraDenied = true;
  });
  await voiceCall(app);
  const camera = app.getByRole("button", { name: "Camera", exact: true });
  await camera.click();
  await expect(app.getByRole("alert")).toContainText("Settings");
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
});

test("a video call whose camera cannot start stays a voice call on a dark screen, and says so", async ({ app }) => {
  await app.addInitScript(() => {
    (window as unknown as { __ftFakeCameraFails: boolean }).__ftFakeCameraFails = true;
  });
  await voiceCall(app, "Video call");
  await expect(app.getByRole("alert")).toContainText("The camera couldn't start");
  await expect(app.locator("html")).not.toHaveClass(/ft-call-video/);
  const screen = app.locator("ion-content.ft-call");
  await expect(screen).toHaveClass(/is-dark/);
  expect(await screen.evaluate((element) => getComputedStyle(element).getPropertyValue("--background").trim())).toBe("#07090c");
  const camera = app.getByRole("button", { name: "Camera", exact: true });
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(camera).toHaveAttribute("aria-disabled", "false");
  await camera.click();
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(app.getByRole("alert")).toContainText("The camera couldn't start");
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
  expect((await callsTo(app)).some(([command]) => command === "core_call_end")).toBe(false);
});

test("going back from the call screen keeps the call and shows the call bar", async ({ app }) => {
  await voiceCall(app);
  await app.goBack();
  await expect(app.locator("ion-content.ft-call")).toHaveCount(0);
  await expect(app.getByTestId("call-bar")).toBeVisible();
  expect((await callsTo(app)).some(([command]) => command === "core_call_end")).toBe(false);
  await expect.poll(async () => lastLayout(app)).toBeNull();
  await app.getByRole("button", { name: "Back to the call" }).click();
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
});

type Ringing = { video?: boolean; answered?: boolean };

async function ring(page: Page, how: Ringing = {}) {
  await page.evaluate((ringing) => (window as unknown as { __ftFake: { ring: (how: Ringing) => void } }).__ftFake.ring(ringing), how);
}

async function nativePhone(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as { __ftFakeNative: boolean }).__ftFakeNative = true;
  });
  await page.goto("/tabs/chats");
  await page.evaluate(() => {
    const seen = window as unknown as { __ftSawRinging: boolean };
    seen.__ftSawRinging = false;
    new MutationObserver(() => {
      if (document.querySelector('[data-test="incoming"]')) seen.__ftSawRinging = true;
    }).observe(document.body, { childList: true, subtree: true });
  });
}

const sawRinging = (page: Page) => page.evaluate(() => (window as unknown as { __ftSawRinging: boolean }).__ftSawRinging);
const answersSent = async (page: Page) => (await callsTo(page)).filter(([command]) => command === "core_call_answer_native").length;

test("an incoming call answered in the app is answered by the core and goes live", async ({ app }) => {
  await nativePhone(app);
  await ring(app);
  await expect(app.getByTestId("incoming")).toBeVisible();
  await app.getByRole("button", { name: "Answer", exact: true }).click();
  await expect(app.getByTestId("incoming")).toHaveCount(0);
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  expect(await answersSent(app)).toBe(1);
});

test("the phone's own call screen answers while the app rings, and the app follows the core", async ({ app }) => {
  await nativePhone(app);
  await ring(app);
  await expect(app.getByTestId("incoming")).toBeVisible();
  await app.evaluate(() => (window as unknown as { __ftFake: { phoneAnswers: () => void } }).__ftFake.phoneAnswers());
  await expect(app.locator(".ft-call__state")).toHaveText("Connecting…");
  await expect(app.getByTestId("incoming")).toHaveCount(0);
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  expect(await answersSent(app)).toBe(0);
});

test("a call answered before its offer came never rings in the app", async ({ app }) => {
  await nativePhone(app);
  await ring(app, { answered: true });
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
  expect(await sawRinging(app)).toBe(false);
  expect(await answersSent(app)).toBe(0);
});
