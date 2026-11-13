import { devices, type Locator } from "@playwright/test";
import { expect, serveRealPluginFrames, test } from "./helpers";

const PROBE = `
customElements.define("ft-probe", class extends HTMLElement {
  connectedCallback() {
    this.innerHTML = '<ion-list><ion-item><ion-toggle id="on" checked>On</ion-toggle></ion-item></ion-list>';
  }
});
`;

const THEMES = ["mono", "ember", "aurora", "lime"].flatMap((direction) => ["dark", "light"].map((appearance) => ({ direction, appearance })));

type Drawn = { mode: string; track: string; knob: string };

function drawn(toggle: Locator): Promise<Drawn> {
  return toggle.evaluate((node) => {
    const part = (name: string) => getComputedStyle(node.shadowRoot!.querySelector(name)!).backgroundColor;
    return { mode: node.classList.contains("ios") ? "ios" : "md", track: part(".toggle-icon"), knob: part(".toggle-inner") };
  });
}

async function settledDrawing(toggle: Locator): Promise<Drawn> {
  let last = await drawn(toggle);
  await expect
    .poll(async () => {
      const now = await drawn(toggle);
      const same = JSON.stringify(now) === JSON.stringify(last);
      last = now;
      return same;
    })
    .toBe(true);
  return last;
}

test.use({ userAgent: devices["iPhone 13"].userAgent, viewport: { width: 390, height: 844 } });

for (const { direction, appearance } of THEMES) {
  test(`a switched-on toggle in a plugin is drawn as the app draws its own on an iPhone, in ${direction} ${appearance}`, async ({ app }) => {
    await app.addInitScript(
      ([chosen, look]) => {
        if (window !== window.top) return;
        localStorage.setItem("ft-direction", chosen);
        localStorage.setItem("ft-appearance", look);
      },
      [direction, appearance],
    );
    await serveRealPluginFrames(app, PROBE);

    await app.goto("/tabs/settings");
    const own = await settledDrawing(app.locator("ion-toggle.toggle-checked").first());

    await app.goto("/chat/ft_bob123456789");
    await app.getByTestId("apps").click();
    await app.getByTestId("app-com.flickertalk.markdown").click();
    const lent = await settledDrawing(app.frameLocator("iframe.ft-plugin__frame").locator("#on.toggle-checked"));

    expect(own.mode).toBe("ios");
    expect(lent.mode).toBe("ios");
    expect(lent.knob, "the knob stands apart from the track").not.toBe(lent.track);
    expect(lent).toEqual(own);
  });
}
