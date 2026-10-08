// Android's navigation bar (2026-10-02, seen on the Samsung with three-button navigation: in
// Settings → Plugins a row sat behind the three buttons). The app is edge to edge, so the bar is
// drawn over the bottom of the WebView, whose height it reports through `env(safe-area-inset-bottom)`.
// On Android the app now ends above the bar: nothing of any page, sheet or window is drawn behind
// it, the strip under it has the page's background, and the screens no longer pad themselves for
// it. iOS (where the home indicator over the content is the norm) and a desktop browser keep
// theirs. The two insets were measured on the real phones (2026-10-02): 48 px on the Samsung S20+
// (Android 13, three buttons, 384 × 853) and 15 px on the Lenovo tablet (Android 16, gestures,
// 1280 × 800).
import type { Page } from "@playwright/test";
import { callsTo, expect, frameSays, test } from "./helpers";

const BOB = "ft_bob123456789";
const BUTTONS = 48;
const GESTURES = 15;
const SAMSUNG = { width: 384, height: 853 };
const TABLET = { width: 1280, height: 800 };
const IPHONE_AGENT =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148";
const DESKTOP_AGENT =
  "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";

/**
 * The inset as the WebView reports it. Ionic reads `--safe-area-inset-bottom` before
 * `env(safe-area-inset-bottom)`, so this plays the WebView without overriding anything the app
 * itself sets (the keyboard's rule, say). Set once the page exists.
 */
async function systemInset(app: Page, inset: number) {
  await app.addInitScript((bottom) => {
    document.addEventListener("DOMContentLoaded", () =>
      document.documentElement.style.setProperty("--safe-area-inset-bottom", `${bottom}px`),
    );
  }, inset);
}

/**
 * What is drawn in the bottom strip the bar covers, sampled across it: every element hit at those
 * points but the document and its body (which paint the page's background). Elements that let taps
 * through are drawn too, so for this they count.
 */
async function drawnInTheStrip(app: Page, inset: number): Promise<string[]> {
  return app.evaluate((bottom) => {
    const style = document.createElement("style");
    style.textContent = "* { pointer-events: auto !important; }";
    document.head.append(style);
    try {
      const { innerWidth: width, innerHeight: height } = window;
      const found = new Set<string>();
      for (const y of [height - bottom + 0.5, height - bottom / 2, height - 0.5]) {
        for (const x of [2, width / 4, width / 2, (3 * width) / 4, width - 2]) {
          for (const element of document.elementsFromPoint(x, y)) {
            if (element === document.documentElement || element === document.body) continue;
            found.add(`${element.tagName.toLowerCase()}.${[...element.classList].slice(0, 3).join(".")}`);
          }
        }
      }
      return [...found];
    } finally {
      style.remove();
    }
  }, inset);
}

/** Nothing of the app in the strip, once whatever moves (a sheet, a page) has settled. */
async function stripIsClear(app: Page, inset: number) {
  await expect.poll(() => drawnInTheStrip(app, inset), { intervals: [100, 200, 300] }).toEqual([]);
}

/**
 * Every scroller on the screen, scrolled to its end: the lowest thing it shows ends wholly above
 * the strip, so the last row can be read and tapped. Clipping inside the scroller counts (a folded
 * part is not shown); the scroller's own edge does not, so a row cut off by the bar fails.
 */
async function lowestRowEnd(app: Page): Promise<number> {
  return app.evaluate(async () => {
    const shown = [...document.querySelectorAll("ion-content")].filter((content) => {
      const box = content.getBoundingClientRect();
      return box.height > 0 && box.width > 0;
    });
    let lowest = 0;
    for (const content of shown) {
      await (content as HTMLIonContentElement).scrollToBottom(0);
    }
    await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    for (const content of shown) {
      for (const element of content.querySelectorAll("*")) {
        const box = element.getBoundingClientRect();
        if (box.height === 0 || box.width === 0 || getComputedStyle(element).visibility === "hidden") continue;
        let top = box.top;
        let bottom = box.bottom;
        for (let parent = element.parentElement; parent && parent !== content; parent = parent.parentElement) {
          const style = getComputedStyle(parent);
          if (style.overflowX === "visible" && style.overflowY === "visible") continue;
          const clip = parent.getBoundingClientRect();
          top = Math.max(top, clip.top);
          bottom = Math.min(bottom, clip.bottom);
        }
        if (bottom > top) lowest = Math.max(lowest, bottom);
      }
    }
    return lowest;
  });
}

async function rowsEndAboveTheStrip(app: Page, inset: number) {
  const height = app.viewportSize()!.height;
  await expect.poll(() => lowestRowEnd(app), { message: "the lowest row, scrolled to the end" }).toBeLessThanOrEqual(height - inset + 0.5);
}

/** The strip's paint: as tall as the bar, in the colour given (an `rgb()` string). */
async function stripPaint(app: Page) {
  return app.evaluate(() => {
    const strip = getComputedStyle(document.body, "::after");
    return { height: strip.height, colour: strip.backgroundColor, content: strip.content };
  });
}

/** The page's background, as the theme sets it. */
async function pageBackground(app: Page) {
  return app.evaluate(() => {
    const probe = document.createElement("div");
    probe.style.background = "var(--ft-bg)";
    document.body.append(probe);
    const colour = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return colour;
  });
}

/** Where the app's box ends, and the bottom padding the composer keeps. */
async function appEnd(app: Page) {
  return app.evaluate(() => Math.round(document.querySelector("ion-app")!.getBoundingClientRect().bottom));
}

async function composerEnd(app: Page) {
  const composer = app.locator(".ft-composer").last();
  await expect(composer).toBeVisible();
  const box = (await composer.boundingBox())!;
  const padding = await composer.evaluate((element) => getComputedStyle(element).paddingBottom);
  return { bottom: Math.round(box.y + box.height), padding };
}

/** Before the app loads: a visual viewport the test shrinks as the on-screen keyboard would (keyboard.spec.ts). */
function installKeyboard() {
  const real = window.visualViewport!;
  let keyboard = 0;
  const fake = new EventTarget();
  Object.defineProperties(fake, {
    height: { get: () => (keyboard ? window.innerHeight - keyboard : real.height) },
    offsetTop: { get: () => 0 },
    width: { get: () => real.width },
    offsetLeft: { get: () => 0 },
    pageTop: { get: () => 0 },
    pageLeft: { get: () => 0 },
    scale: { get: () => 1 },
  });
  real.addEventListener("resize", () => fake.dispatchEvent(new Event("resize")));
  Object.defineProperty(window, "visualViewport", { value: fake, configurable: true });
  (window as unknown as { __keyboard: (height: number) => void }).__keyboard = (height) => {
    keyboard = height;
    fake.dispatchEvent(new Event("resize"));
  };
}

/** Every screen reachable by its address, with what it needs before it opens. */
const SCREENS: Array<{ path: string; many?: boolean }> = [
  { path: "/tabs/chats" },
  { path: "/tabs/calls" },
  { path: "/tabs/games", many: true },
  { path: "/tabs/settings" },
  { path: "/plugins", many: true },
  { path: "/blocked" },
  { path: "/hours" },
  { path: "/session" },
  { path: "/move?role=old" },
  { path: "/backup" },
  { path: `/contact/${BOB}` },
  { path: "/add-contact" },
  { path: "/new-circle" },
  { path: "/circle/circle1/info" },
  { path: "/circle/circle1" },
  { path: `/chat/${BOB}` },
  { path: "/plugin/com.flickertalk.markdown" },
];

for (const [name, size, inset] of [
  ["the Samsung (three buttons)", SAMSUNG, BUTTONS],
  ["the tablet (gestures)", TABLET, GESTURES],
] as const) {
  test.describe(`on Android, ${name}`, () => {
    test.use({ viewport: size });
    test.beforeEach(async ({ app }) => {
      await systemInset(app, inset);
    });

    for (const screen of SCREENS) {
      test(`${screen.path}: nothing is drawn behind the bar and its last row ends above it`, async ({ app }) => {
        if (screen.many) await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeManyPlugins = 12));
        await app.goto(screen.path);
        await expect(app.locator("ion-content:visible").first()).toBeVisible();
        await stripIsClear(app, inset);
        await rowsEndAboveTheStrip(app, inset);
        // The app's box ends where the bar starts, and the strip is painted with the page's background.
        expect(await appEnd(app)).toBe(size.height - inset);
        const strip = await stripPaint(app);
        expect(strip.height).toBe(`${inset}px`);
        expect(strip.colour).toBe(await pageBackground(app));
      });
    }

    test("the composer sits right above the bar, with no room kept for it twice", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await stripIsClear(app, inset);
      await expect.poll(() => composerEnd(app)).toEqual({ bottom: size.height - inset, padding: "8px" });
    });

    test("with the keyboard open the composer sits right on it, and the strip is gone", async ({ app }) => {
      await app.addInitScript(installKeyboard);
      await app.goto(`/chat/${BOB}`);
      await expect.poll(() => composerEnd(app)).toEqual({ bottom: size.height - inset, padding: "8px" });
      await app.locator(".ft-composer__input textarea").click();
      await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(330));
      // The keyboard covers the bar: the app ends on the keyboard, no gap and no inset under the composer.
      await expect.poll(() => composerEnd(app)).toEqual({ bottom: size.height - 330, padding: "8px" });
      expect(await appEnd(app)).toBe(size.height - 330);
      await app.evaluate(() => (window as unknown as { __keyboard: (height: number) => void }).__keyboard(0));
      await expect.poll(() => composerEnd(app)).toEqual({ bottom: size.height - inset, padding: "8px" });
    });

    test("the chat's apps sheet, a plugin's window and the permissions sheet stay above the bar", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await app.getByTestId("apps").click();
      await expect(app.getByTestId("app-com.flickertalk.sketch")).toBeVisible();
      await stripIsClear(app, inset);
      await rowsEndAboveTheStrip(app, inset);

      await app.getByTestId("apps-tab-games").click();
      await expect(app.getByTestId("more-games-link")).toBeVisible();
      await stripIsClear(app, inset);

      await app.getByTestId("apps-tab-tools").click();
      await app.getByTestId("app-com.flickertalk.markdown").click();
      await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
      await frameSays(app, { type: "ft.height", height: 1400 });
      await expect.poll(async () => (await app.locator("iframe.ft-plugin__frame").boundingBox())?.height).toBe(1400);
      await app.evaluate(() => {
        const scroller = document.querySelector(".ft-app__body");
        if (scroller) scroller.scrollTop = scroller.scrollHeight;
      });
      await stripIsClear(app, inset);
      const frame = (await app.locator("iframe.ft-plugin__frame").boundingBox())!;
      expect(frame.y + frame.height).toBeLessThanOrEqual(size.height - inset + 0.5);
    });

    test("the games' permissions sheet and the game room stay above the bar", async ({ app }) => {
      await app.goto(`/chat/${BOB}`);
      await app.getByTestId("apps").click();
      await app.getByTestId("apps-tab-games").click();
      await app.getByTestId("game-com.flickertalk.game.tictactoe").click();
      await expect(app.getByTestId("game-allow")).toBeVisible();
      await stripIsClear(app, inset);
      await app.getByTestId("game-allow").click();
      await expect(app.getByTestId("game-room")).toBeVisible();
      await stripIsClear(app, inset);
      await expect.poll(async () => (await composerEnd(app)).bottom).toBe(size.height - inset);
    });

    test("the suggestions modal stays above the bar", async ({ app }) => {
      await app.goto("/tabs/settings");
      await app.getByTestId("feedback").click();
      await expect(app.locator("ion-modal ion-textarea")).toBeVisible();
      await stripIsClear(app, inset);
    });

    test("a ringing call, the call screen and its pictures stay above the bar", async ({ app }) => {
      await app.addInitScript(() => ((window as unknown as Record<string, unknown>).__ftFakeNative = true));
      await app.goto("/tabs/chats");
      await app.evaluate(() => (window as unknown as { __ftFake: { ring: (how: object) => void } }).__ftFake.ring({}));
      await expect(app.getByTestId("incoming")).toBeVisible();
      await stripIsClear(app, inset);
      await app.getByRole("button", { name: "Answer", exact: true }).click();
      await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
      await stripIsClear(app, inset);
      await rowsEndAboveTheStrip(app, inset);

      // My camera on: the page turns see-through around the pictures, the strip stays painted.
      await app.getByRole("button", { name: "Camera", exact: true }).click();
      await expect(app.locator("html")).toHaveClass(/ft-call-video/);
      await stripIsClear(app, inset);
      expect((await stripPaint(app)).height).toBe(`${inset}px`);
    });

    test("the camera scanner stays above the bar", async ({ app }) => {
      await app.goto("/add-contact");
      await app.getByTestId("mode-scan").click();
      await app.getByTestId("scan-now").click();
      await expect(app.getByTestId("scanner-overlay")).toBeVisible();
      await stripIsClear(app, inset);
      expect((await stripPaint(app)).height).toBe(`${inset}px`);
    });
  });
}

// iOS keeps the home indicator over the content (the platform's norm) and a desktop browser has
// no bar: for them nothing changes, the screens still pad themselves for the inset.
for (const [name, agent] of [
  ["an iPhone", IPHONE_AGENT],
  ["a desktop browser", DESKTOP_AGENT],
] as const) {
  test.describe(`on ${name}`, () => {
    test.use({ viewport: { width: 390, height: 844 }, userAgent: agent });

    test("the app takes the whole screen and the composer keeps room for the inset", async ({ app }) => {
      await systemInset(app, 34);
      await app.goto(`/chat/${BOB}`);
      await expect.poll(() => composerEnd(app)).toEqual({ bottom: 844, padding: "42px" });
      expect(await appEnd(app)).toBe(844);
      expect((await stripPaint(app)).content).toBe("none");
    });
  });
}

// The system bars' icons (2026-10-02): white on the light app until now. The page tells the phone
// whether it is dark (`core_system_bars`, the bridge's `setSystemBars` on Android) when it starts
// and each time that changes, and the strip under the navigation bar has the same background.
test.describe("the system bars follow the app", () => {
  test.use({ viewport: SAMSUNG });
  test.beforeEach(async ({ app }) => {
    await systemInset(app, BUTTONS);
  });

  /** What the bars were last told: dark or not. */
  async function barsTold(app: Page) {
    const told = (await callsTo(app)).filter(([command]) => command === "core_system_bars");
    return told.at(-1)?.[1]?.dark;
  }

  test("dark from the start, light once the light appearance is chosen, and back", async ({ app }) => {
    await app.goto("/tabs/settings");
    await expect.poll(() => barsTold(app)).toBe(true);
    expect((await stripPaint(app)).colour).toBe(await pageBackground(app));

    await app.getByRole("button", { name: "Light", exact: true }).click();
    await expect.poll(() => barsTold(app)).toBe(false);
    await expect.poll(() => stripPaint(app).then((strip) => strip.colour)).toBe(await pageBackground(app));
    expect(await pageBackground(app)).not.toBe("rgb(0, 0, 0)");

    await app.getByRole("button", { name: "Dark", exact: true }).click();
    await expect.poll(() => barsTold(app)).toBe(true);
  });

  test("on the light app, a call's pictures turn the bars and the strip dark while they show", async ({ app }) => {
    await app.addInitScript(() => {
      localStorage.setItem("ft-appearance", "light");
      (window as unknown as Record<string, unknown>).__ftFakeNative = true;
    });
    await app.goto(`/chat/${BOB}`);
    await expect.poll(() => barsTold(app)).toBe(false);
    await app.getByRole("button", { name: "Voice call", exact: true }).click();
    await expect(app.locator(".ft-call__state")).toHaveText(/^00:0\d$/);
    // A voice call sits on the page's own background.
    expect(await barsTold(app)).toBe(false);

    await app.getByRole("button", { name: "Camera", exact: true }).click();
    await expect(app.locator("html")).toHaveClass(/ft-call-video/);
    await expect.poll(() => barsTold(app)).toBe(true);
    expect((await stripPaint(app)).colour).toBe("rgb(7, 9, 12)");

    await app.getByRole("button", { name: "Camera", exact: true }).click();
    await expect.poll(() => barsTold(app)).toBe(false);
    expect((await stripPaint(app)).colour).toBe(await pageBackground(app));
  });

  test("on the light app, the camera scanner turns the bars and the strip dark until it closes", async ({ app }) => {
    await app.addInitScript(() => localStorage.setItem("ft-appearance", "light"));
    await app.goto("/add-contact");
    await expect.poll(() => barsTold(app)).toBe(false);
    await app.getByTestId("mode-scan").click();
    await app.getByTestId("scan-now").click();
    await expect(app.getByTestId("scanner-overlay")).toBeVisible();
    await expect.poll(() => barsTold(app)).toBe(true);
    expect((await stripPaint(app)).colour).toBe("rgb(7, 9, 12)");

    await app.getByTestId("scanner-overlay").getByRole("button", { name: "Cancel" }).click();
    await expect(app.getByTestId("scanner-overlay")).toBeHidden();
    await expect.poll(() => barsTold(app)).toBe(false);
  });
});
