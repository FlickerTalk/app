// The quick tour (2026-10-28, Ioan: "the typical quick tour"): a new identity sees it once, when the
// chats open after Start; an install set up before never does on its own; Settings and the empty
// chat list open it again. Ionic's sheet modal with Ionic's swipeable segment view: a swipe brings
// the next step from the inline end (the right in English, the left in Arabic).
import type { Page } from "@playwright/test";
import { expect, test } from "./helpers";

const CARD = "https://flickertalk.com/add#oWR2ZXJzaW9uAWRuYW1lY0FuYQ";

/** A first run: the welcome not seen yet, once per tab, so a reload keeps what Start did. */
function firstRun() {
  if (sessionStorage.getItem("ft-e2e-first-run")) return;
  sessionStorage.setItem("ft-e2e-first-run", "1");
  localStorage.removeItem("ft-onboarded");
}

/** A phone with no conversations, requests or circles yet. */
function noChats() {
  const core = (window as unknown as { __TAURI_INTERNALS__: { invoke: (...args: unknown[]) => Promise<unknown> } }).__TAURI_INTERNALS__;
  const invoke = core.invoke.bind(core);
  const EMPTY = new Set(["core_conversations", "core_requests", "core_circles"]);
  core.invoke = (command: unknown, ...rest: unknown[]) => (EMPTY.has(String(command)) ? Promise.resolve([]) : invoke(command, ...rest));
}

/** The tour on screen: Chats and Settings each hold one, which Ionic hides while it is shut. */
const tour = (app: Page) => app.locator("[data-test='quick-tour']:not(.overlay-hidden)");
const steps = (app: Page) => tour(app).getByTestId("tour-step");
const heading = (app: Page, title: string) => tour(app).getByRole("heading", { name: title });

/**
 * Screenshots for review, only when asked for: `FT_SHOTS=<dir> npx playwright test
 * e2e/quick-tour.spec.ts`.
 */
async function shot(app: Page, name: string) {
  const dir = process.env.FT_SHOTS;
  if (!dir) return;
  await app.waitForTimeout(450);
  await app.screenshot({ path: `${dir}/${name}.png` });
}

/** Whatever Ionic still does after the page is drawn (a tour would be up by then). */
async function settled(app: Page) {
  await expect(app.locator("[data-test='chat-row'], [data-test='empty']").first()).toBeVisible();
  await app.waitForTimeout(800);
}

test("a new identity sees the tour once Start opens the chats, and only once", async ({ app }) => {
  await app.addInitScript(firstRun);
  await app.goto("/");
  await expect(app).toHaveURL(/\/welcome$/);
  await app.getByTestId("start").click();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect(heading(app, "Add a contact")).toBeInViewport();
  await shot(app, "first-run-en");
  await tour(app).getByTestId("tour-next").click();
  await expect(heading(app, "Write and send")).toBeInViewport();
  await tour(app).getByTestId("tour-skip").click();
  await expect(tour(app)).toHaveCount(0);

  // Another tab and back, and a restart: it does not come back on its own.
  await app.getByRole("tab", { name: "Settings" }).click();
  await expect(app).toHaveURL(/\/tabs\/settings$/);
  await app.getByRole("tab", { name: "Chats" }).click();
  await settled(app);
  await expect(tour(app)).toHaveCount(0);
  await app.reload();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await settled(app);
  await expect(tour(app)).toHaveCount(0);
});

test("an install set up before never sees it on its own", async ({ app }) => {
  await app.goto("/tabs/chats");
  await settled(app);
  await expect(tour(app)).toHaveCount(0);
});

// The link that opened the app on its first run opens Add contact after Start: the tour waits for
// the chats instead of covering that page.
test("on a first run that came with a contact link, the tour waits for the chats", async ({ app }) => {
  await app.addInitScript(firstRun);
  await app.addInitScript((link) => void ((window as unknown as Record<string, unknown>).__ftFakeOpenedLink = link), CARD);
  await app.goto("/");
  await app.getByTestId("start").click();
  await expect(app).toHaveURL(/\/add-contact$/);
  await expect(app.getByTestId("paste")).toHaveValue(CARD);
  await app.waitForTimeout(800);
  await expect(tour(app)).toHaveCount(0);
  await app.goBack();
  await expect(app).toHaveURL(/\/tabs\/chats$/);
  await expect(heading(app, "Add a contact")).toBeInViewport();
});

test("Settings opens it again, Next walks every step and Done closes it", async ({ app }) => {
  await app.goto("/tabs/settings");
  await app.getByTestId("tour").click();
  await expect(heading(app, "Add a contact")).toBeInViewport();
  const next = tour(app).getByTestId("tour-next");
  for (const title of [
    "Write and send",
    "Voice and video calls",
    "Tools and games",
    "Block and report",
    "Privacy",
    "The FlickerTalk contact",
    "A new phone",
  ]) {
    await next.click();
    await expect(heading(app, title)).toBeInViewport();
  }
  await expect(next).toHaveText("Done");
  await expect(tour(app).getByTestId("tour-skip")).toBeVisible();
  await next.click();
  await expect(tour(app)).toHaveCount(0);
  await expect(app).toHaveURL(/\/tabs\/settings$/);

  await app.getByTestId("tour").click();
  await expect(heading(app, "Add a contact")).toBeInViewport();
});

test("the empty chat list links to the tour", async ({ app }) => {
  await app.addInitScript(noChats);
  await app.goto("/tabs/chats");
  await expect(app.getByTestId("empty")).toBeVisible();
  const link = app.getByTestId("tour-link");
  await expect(link).toHaveText("How FlickerTalk works");
  await shot(app, "empty-list-link-en");
  await link.click();
  await expect(heading(app, "Add a contact")).toBeInViewport();
  await tour(app).getByTestId("tour-skip").click();
  await expect(tour(app)).toHaveCount(0);
  await expect(app.getByTestId("empty")).toBeVisible();
});

for (const locale of ["en", "ar"] as const) {
  test.describe(locale, () => {
    test.use({ locale });

    test("a swipe brings the next step from the inline end, and the dots follow", async ({ app }) => {
      await app.goto("/tabs/settings");
      await app.getByTestId("tour").click();
      const first = steps(app).nth(0);
      const second = steps(app).nth(1);
      await expect(first).toBeInViewport();
      // Once Ionic has brought the sheet all the way up.
      let last = "";
      await expect
        .poll(async () => {
          const now = JSON.stringify(await first.boundingBox());
          const still = now === last;
          last = now;
          return still;
        }, { intervals: [150] })
        .toBe(true);
      const at = (await first.boundingBox())!;
      const after = (await second.boundingBox())!;
      // The next step waits at the inline end: on the right in English, on the left in Arabic.
      if (locale === "ar") expect(after.x).toBeLessThan(at.x);
      else expect(after.x).toBeGreaterThan(at.x);

      // A finger swipe across the step (towards the inline start), as a touch screen sends it.
      const cdp = await app.context().newCDPSession(app);
      await cdp.send("Input.synthesizeScrollGesture", {
        x: Math.round(at.x + at.width / 2),
        y: Math.round(at.y + at.height / 2),
        xDistance: locale === "ar" ? 260 : -260,
        yDistance: 0,
        gestureSourceType: "touch",
        speed: 1200,
      });
      await expect(second).toBeInViewport({ ratio: 0.9 });
      // The dots follow; each is a tab named for screen readers (Ionic puts the name on its button).
      await expect(tour(app).locator("ion-segment-button").nth(1)).toHaveClass(/segment-button-checked/);
      const dot = locale === "ar" ? "الخطوة 2 من 8" : "Step 2 of 8";
      await expect(tour(app).getByRole("tab", { name: dot, selected: true })).toHaveCount(1);
      // A sideways swipe moves the steps, not the sheet.
      await expect(tour(app)).toBeVisible();
    });
  });
}

// Screenshots for review in English, Spanish and Arabic (RTL), on a phone and on a tablet.
for (const locale of ["en", "es", "ar"] as const) {
  test.describe(`screens, ${locale}`, () => {
    test.use({ locale });
    test.skip(!process.env.FT_SHOTS, "only when screenshots are asked for");

    test("the steps on a phone", async ({ app }) => {
      await app.goto("/tabs/settings");
      await app.getByTestId("tour").click();
      await expect(steps(app).nth(0)).toBeInViewport();
      await shot(app, `phone-${locale}-1`);
      const next = tour(app).getByTestId("tour-next");
      for (let step = 1; step < 8; step++) {
        await next.click();
        await expect(steps(app).nth(step)).toBeInViewport({ ratio: 0.9 });
        if (step === 5 || step === 7) await shot(app, `phone-${locale}-${step + 1}`);
      }
    });

    test("the steps on a tablet", async ({ app }) => {
      await app.setViewportSize({ width: 1280, height: 800 });
      await app.goto("/tabs/settings");
      await app.getByTestId("tour").click();
      await expect(steps(app).nth(0)).toBeInViewport();
      await shot(app, `tablet-${locale}-1`);
    });
  });
}
