// 2026-10-03 (updates of downloaded plugins): the core updates what the user downloaded in the
// background, never under an open frame. The screens tell it which plugins are open, and read the
// list again when it says the plugins changed (`ft://plugins`).
import { callsTo, expect, servePluginFrames, test } from "./helpers";

const SKETCH = "com.flickertalk.sketch";

test("a plugin updated in the background shows its new version on its sheet at once", async ({ app }) => {
  await app.goto("/tabs/apps");
  await app.getByTestId(`app-${SKETCH}`).click({ button: "right" });
  const page = app.getByTestId("sheet-meta");
  await expect(page).toHaveText("1.0.0");
  await app.evaluate((id) => {
    const fake = (window as unknown as { __ftFake: { state: { plugins: Array<{ id: string; version: string }> }; emit: (event: string, payload: unknown) => void } }).__ftFake;
    fake.state.plugins.find((one) => one.id === id)!.version = "1.0.1";
    fake.emit("ft://plugins", null);
  }, SKETCH);
  await expect(page).toHaveText("1.0.1");
});

test("the core hears that a plugin is open until its frame is gone", async ({ app }) => {
  await servePluginFrames(app);
  await app.goto("/chat/ft_bob123456789");
  await app.getByTestId("apps").click();
  await app.getByTestId(`app-${SKETCH}`).click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(1);
  // The frame asks for the version installed.
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveAttribute("src", /\/frame\.html\?v=1\.0\.0$/);
  const opens = async () => (await callsTo(app)).filter(([command]) => command === "core_plugin_open").map(([, args]) => args);
  await expect.poll(opens).toEqual([{ plugin: SKETCH, open: true }]);

  await app.getByTestId("close-app").click();
  await expect(app.locator("iframe.ft-plugin__frame")).toHaveCount(0);
  await expect.poll(opens).toEqual([
    { plugin: SKETCH, open: true },
    { plugin: SKETCH, open: false },
  ]);
});
