// Native video (2026-09-29, docs/video-nativo.md): on a phone every call can go from voice to
// video and back at any moment. The fake core plays a phone (`__ftFakeNative`): calls are
// native, and what the other side's camera does is emitted as the core would.
import type { Page } from "@playwright/test";
import { callsTo, expect, test } from "./helpers";

type Fake = { emit: (event: string, payload: unknown) => void };

/** Where the UI last told the core the pictures go. */
async function lastLayout(page: Page) {
  const layouts = (await callsTo(page)).filter(([command]) => command === "core_call_video_layout");
  return layouts.at(-1)?.[1]?.layout;
}

/** The other side's camera, as the core's snapshot says it. */
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
  // Connected: the call's clock runs.
  await expect(page.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
}

test("a voice call goes to video and back, from either side", async ({ app }) => {
  await voiceCall(app);
  expect((await callsTo(app)).find(([command]) => command === "core_call_start_native")?.[1]).toMatchObject({ video: false });
  const camera = app.getByRole("button", { name: "Camera", exact: true });
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  // Always there on a phone, inert while my camera is off (2026-09-29).
  await expect(app.getByRole("button", { name: "Switch camera" })).toHaveAttribute("aria-disabled", "true");

  // My camera on: my picture fills the screen, the page turns see-through around it.
  await camera.click();
  await expect(camera).toHaveAttribute("aria-pressed", "true");
  await expect(app.getByTestId("local-slot")).toBeVisible();
  await expect(app.locator("html")).toHaveClass(/ft-call-video/);
  await expect.poll(async () => (await lastLayout(app))?.local).not.toBeNull();
  await app.getByRole("button", { name: "Switch camera" }).click();
  await expect.poll(async () => (await lastLayout(app))?.mirrorLocal).toBe(false);

  // Theirs on too: their picture fills the screen and mine becomes the thumbnail.
  await theirCamera(app, true);
  await expect(app.getByTestId("remote-slot")).toBeVisible();
  await expect(app.getByTestId("local-slot")).toHaveClass(/is-thumb/);
  await expect.poll(async () => (await lastLayout(app))?.remote).not.toBeNull();

  // Their phone locks: their camera is held.
  await theirCamera(app, true, true);
  await expect(app.getByTestId("remote-paused")).toContainText("Camera paused");

  // Both off again: back to a voice call.
  await camera.click();
  await theirCamera(app, false);
  await expect(app.getByTestId("remote-slot")).toHaveCount(0);
  await expect(app.getByTestId("local-slot")).toHaveCount(0);
  await expect(app.locator("html")).not.toHaveClass(/ft-call-video/);
  await expect.poll(async () => lastLayout(app)).toMatchObject({ remote: null, local: null });

  // Leaving the screen hides the pictures.
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

// Found by QA on the emulators (2026-09-29): a video call whose camera failed to start left a
// blank white screen, with the camera button stuck on. The core says nothing when that happens.
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
  // The camera shows as the core has it: off, and it can be tried again.
  const camera = app.getByRole("button", { name: "Camera", exact: true });
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(camera).toHaveAttribute("aria-disabled", "false");
  await camera.click();
  await expect(camera).toHaveAttribute("aria-pressed", "false");
  await expect(app.getByRole("alert")).toContainText("The camera couldn't start");
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
  expect((await callsTo(app)).some(([command]) => command === "core_call_end")).toBe(false);
});

// Found by QA on the emulators (2026-09-29): the back gesture hung up. Leaving the call screen
// keeps the call and shows the call bar; only the hang-up button ends it.
test("going back from the call screen keeps the call and shows the call bar", async ({ app }) => {
  await voiceCall(app);
  await app.goBack();
  // The call screen is gone for good (Ionic removes it after its transition), the call is not.
  await expect(app.locator("ion-content.ft-call")).toHaveCount(0);
  await expect(app.getByTestId("call-bar")).toBeVisible();
  expect((await callsTo(app)).some(([command]) => command === "core_call_end")).toBe(false);
  await expect.poll(async () => lastLayout(app)).toBeNull();
  await app.getByRole("button", { name: "Back to the call" }).click();
  await expect(app.locator(".ft-call__state")).toHaveText(/^00:\d\d$/);
});
