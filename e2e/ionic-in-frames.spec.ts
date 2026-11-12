import type { ConsoleMessage, Page } from "@playwright/test";
import { expect, serveRealPluginFrames, test } from "./helpers";

const PROBE = `
customElements.define("ft-probe", class extends HTMLElement {
  connectedCallback() {
    this.style.cssText = "display:flex;flex-direction:column;height:100%";
    this.innerHTML = '<ion-header><ion-toolbar><ion-title>Probe</ion-title></ion-toolbar></ion-header>' +
      '<ion-content style="flex:1"><ion-list>' +
      Array.from({ length: 40 }, (_, at) => '<ion-item><ion-label>Row ' + at + '</ion-label></ion-item>').join("") +
      '</ion-list><ion-button id="save" color="primary"><ion-icon slot="start" name="trash-outline"></ion-icon>Save</ion-button></ion-content>';
  }
});
ft.onOpen(async () => {
  const toast = document.createElement("ion-toast");
  toast.message = "Saved";
  toast.id = "toast";
  document.body.appendChild(toast);
  await toast.present();
  const alert = document.createElement("ion-alert");
  alert.header = "Delete?";
  alert.id = "alert";
  alert.buttons = [{ text: "Cancel", role: "cancel" }, { text: "Delete", role: "destructive" }];
  document.body.appendChild(alert);
  await alert.present();
  const { role } = await alert.onDidDismiss();
  document.body.dataset.answer = role;
});
`;

async function openProbe(app: Page) {
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId("app-com.flickertalk.markdown").click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  return app.frameLocator("iframe.ft-plugin__frame");
}

test("a plugin draws Ionic's components and overlays with the Ionic the app lends it", async ({ app }) => {
  const problems: string[] = [];
  app.on("console", (message: ConsoleMessage) => {
    if (message.type() === "error" || /Content Security Policy|Refused/.test(message.text())) problems.push(message.text());
  });
  app.on("pageerror", (error) => problems.push(error.message));
  await app.addInitScript(() => {
    if (window === window.top) localStorage.setItem("ft-appearance", "dark");
  });
  await serveRealPluginFrames(app, PROBE);
  const frame = await openProbe(app);

  const save = frame.locator("#save");
  await expect(save).toBeVisible();
  const native = await save.evaluate((node) => getComputedStyle(node.shadowRoot!.querySelector(".button-native")!).backgroundColor);
  const accent = await app.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.color = "var(--ion-color-primary)";
    document.body.append(probe);
    const seen = getComputedStyle(probe).color;
    probe.remove();
    return seen;
  });
  expect(native).toBe(accent);
  await expect.poll(() => frame.locator("#save ion-icon").evaluate((icon) => Boolean(icon.shadowRoot?.querySelector("svg")))).toBe(true);

  const root = await frame.locator("html").evaluate((html) => ({
    ionic: html.dataset.ionic,
    mode: html.getAttribute("mode"),
    fill: html.dataset.fill,
    textRgb: getComputedStyle(html).getPropertyValue("--ion-text-color-rgb").trim(),
  }));
  expect(root).toMatchObject({ ionic: "9.0.4", mode: "md", fill: "1" });
  expect(root.textRgb).toMatch(/^\d+, \d+, \d+$/);

  const pane = (await app.locator(".ft-app__body").boundingBox())!;
  const tall = (await app.locator("iframe.ft-plugin__frame").boundingBox())!;
  expect(Math.abs(tall.y + tall.height - (pane.y + pane.height))).toBeLessThan(2);
  expect(tall.height).toBeGreaterThan(pane.height * 0.9);

  const viewport = app.viewportSize()!;
  for (const id of ["#toast", "#alert"]) {
    const overlay = frame.locator(id);
    await expect(overlay).toBeVisible();
    const shown = await overlay.evaluate((node) => {
      const part = (node.shadowRoot?.querySelector(".toast-wrapper, .alert-wrapper") ?? node) as HTMLElement;
      const box = part.getBoundingClientRect();
      return { top: box.top, bottom: box.bottom, height: box.height };
    });
    expect(shown.height, id).toBeGreaterThan(0);
    expect(tall.y + shown.top, id).toBeGreaterThanOrEqual(0);
    expect(tall.y + shown.bottom, id).toBeLessThanOrEqual(viewport.height + 1);
  }
  await frame.locator("#alert button", { hasText: "Delete" }).click();
  await expect(frame.locator("body")).toHaveAttribute("data-answer", "destructive");

  expect(problems).toEqual([]);
});

test("a plugin opened on its own page is as tall as the page under its bar", async ({ app }) => {
  await serveRealPluginFrames(app, PROBE);
  await app.goto("/plugin/com.flickertalk.markdown");
  const frame = app.locator("iframe.ft-plugin__frame");
  await expect(frame).toHaveCount(1);
  await expect(app.frameLocator("iframe.ft-plugin__frame").locator("#save")).toBeAttached();
  const page = (await app.locator("ion-content.ft-plugin-page").boundingBox())!;
  const tall = (await frame.boundingBox())!;
  expect(tall.height).toBeGreaterThan(page.height * 0.9);
  expect(tall.y + tall.height).toBeLessThanOrEqual(page.y + page.height + 1);
});

test("a plugin's header is not pushed down by the screen's insets inside its frame", async ({ app }) => {
  await serveRealPluginFrames(app, PROBE);
  await app.goto("/plugin/com.flickertalk.markdown");
  await expect(app.frameLocator("iframe.ft-plugin__frame").locator("#save")).toBeAttached();
  const frame = app.frames().find((one) => one !== app.mainFrame())!;
  const header = () => frame.evaluate(() => document.querySelector("ion-header")!.getBoundingClientRect().height);
  await expect.poll(header).toBeGreaterThan(0);
  const before = await header();

  await frame.evaluate(() => {
    document.documentElement.style.setProperty("--safe-area-inset-top", "50px");
    document.documentElement.style.setProperty("--safe-area-inset-bottom", "34px");
  });

  await expect.poll(header).toBe(before);
  const insets = await frame.evaluate(() => {
    const style = getComputedStyle(document.documentElement);
    return [style.getPropertyValue("--ion-safe-area-top").trim(), style.getPropertyValue("--ion-safe-area-bottom").trim()];
  });
  expect(insets).toEqual(["0px", "0px"]);
});
