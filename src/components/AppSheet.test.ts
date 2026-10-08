import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton, IonIcon, IonToggle } from "@ionic/vue";
import { imageOutline, lockClosedOutline, openOutline, play, trashOutline } from "ionicons/icons";
import AppSheet from "./AppSheet.vue";
import source from "./AppSheet.vue?raw";
import { IonModalStub } from "../__tests__/ionic";
import { installed, offered, pluginImage, premiumLocked } from "../plugins";
import type { OfferedPlugin, PluginView } from "../core";

const IMAGE: PluginView = {
  id: "com.flickertalk.images",
  name: "Image",
  version: "1.0.1",
  icon: "image-outline",
  asks: { network: [], messages: true, send: "propose" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
};
const QUIET: PluginView = { ...IMAGE, id: "com.flickertalk.quiet", name: "Quiet", asks: { network: [], messages: false, send: "nothing" } };
const CHESS: PluginView = {
  id: "com.flickertalk.game.chess",
  name: "Chess",
  version: "1.0.0",
  kind: "game",
  asks: { network: [], messages: false, send: "propose", live: true },
  granted: { network: [], messages: false, send: "propose", live: true },
  installedAt: 2,
};
const offer = (one: Partial<OfferedPlugin> & { id: string; name: string }): OfferedPlugin => ({
  version: "1.0.1",
  summary: `${one.name} does a thing.`,
  size: 48_000,
  installed: false,
  carried: false,
  ...one,
});

const sheet = (plugin: PluginView | OfferedPlugin | null, props: Record<string, unknown> = {}) =>
  mount(AppSheet, { props: { plugin, open: Boolean(plugin), ...props }, global: { stubs: { IonModal: IonModalStub } } });

const html = (wrapper: ReturnType<typeof sheet>) => wrapper.find("[data-test='app-sheet']").html();

describe("AppSheet", () => {
  beforeEach(() => {
    offered.value = [offer({ id: IMAGE.id, name: "Image", installed: true, summary: "Shrinks and crops a picture." })];
    installed.value = [IMAGE, QUIET, CHESS];
  });
  afterEach(() => {
    premiumLocked.value = false;
    offered.value = [];
    installed.value = [];
  });

  // 2026-10-08 (plan of the apps grid, screen 3): Ionic's sheet modal, as the chat's apps sheet.
  // Device review of app#121 (2026-10-08): at a fixed 60 % the sheet was half empty, and on the
  // iPhone a game's remove question ran under the home indicator. As tall as what it says, as the
  // game's permissions sheet; taller than the screen, it scrolls inside (a scroll host of Ionic's,
  // so a drag there scrolls rather than moving the sheet).
  it("is Ionic's sheet modal, as tall as what it says, scrolling inside when that is too tall", () => {
    const wrapper = sheet(IMAGE);
    const modal = wrapper.findComponent(IonModalStub);
    expect(modal.props("isOpen")).toBe(true);
    expect(modal.props("breakpoints")).toEqual([0, 1]);
    expect(modal.props("initialBreakpoint")).toBe(1);
    expect(modal.classes()).toContain("ft-sheet--tab");
    expect(modal.attributes("aria-label")).toBe("Image");
    // No ion-content: it has no height of its own, and an auto-height sheet would be empty.
    expect(wrapper.find("ion-content").exists()).toBe(false);
    expect(wrapper.find("[data-test='app-sheet']").classes()).toContain("ion-content-scroll-host");
    // Its styles: an auto height, a body that scrolls within the screen, and the bottom inset.
    const styles = source.slice(source.indexOf("<style"));
    expect(styles).toMatch(/\.ft-app-sheet\s*\{[^}]*--height:\s*auto/);
    expect(styles).toMatch(/\.ft-app-sheet__body\s*\{[^}]*overflow-y:\s*auto/);
    expect(styles).toMatch(/\.ft-app-sheet__body\s*\{[^}]*max-height:/);
    expect(styles).toMatch(/\.ft-app-sheet__body\s*\{[^}]*padding-bottom:[^;]*--ion-safe-area-bottom/);
    // Second device review: Ionic sets --ion-safe-area-top to 0 inside a modal, so a tall sheet
    // reached under the status bar. The system's own inset counts too, at the top and the bottom.
    expect(styles).toMatch(/\.ft-app-sheet__body\s*\{[^}]*max-height:[^;]*env\(safe-area-inset-top/);
    expect(styles).toMatch(/\.ft-app-sheet__body\s*\{[^}]*padding-bottom:[^;]*env\(safe-area-inset-bottom/);
  });

  it("shows nothing while closed", () => {
    expect(sheet(null).find("[data-test='app-sheet']").exists()).toBe(false);
  });

  it("says what an installed tool is: its icon, name, version and weight, and what it does", () => {
    const wrapper = sheet(IMAGE);
    const box = wrapper.find(".ft-app-sheet__icon");
    expect(box.findComponent(IonIcon).props("icon")).toBe(imageOutline);
    expect(wrapper.find("h2").text()).toBe("Image");
    expect(wrapper.find("[data-test='sheet-meta']").text()).toBe("1.0.1 · 48 KB");
    expect(wrapper.find("[data-test='sheet-summary']").text()).toBe("Shrinks and crops a picture.");
  });

  it("leaves the weight out when it is not known", () => {
    offered.value = [];
    const wrapper = sheet(IMAGE);
    expect(wrapper.find("[data-test='sheet-meta']").text()).toBe("1.0.1");
    expect(wrapper.find("[data-test='sheet-summary']").exists()).toBe(false);
  });

  // §53: every permission it asked for, on its own switch; the page decides what a switch does.
  it("has a switch for each permission it asked for, and says when it asks for nothing", async () => {
    const wrapper = sheet(IMAGE);
    const toggles = wrapper.findAllComponents(IonToggle);
    expect(toggles.map((one) => one.attributes("aria-label"))).toEqual(["Read what you send it", "Write in the chat"]);
    expect(toggles.map((one) => one.props("checked"))).toEqual([true, false]);
    toggles[1].vm.$emit("ionChange", { detail: { checked: true } });
    const [key, event] = wrapper.emitted("toggle")![0] as [string, { detail: { checked: boolean } }];
    expect(key).toBe("send");
    expect(event.detail.checked).toBe(true);

    const quiet = sheet(QUIET);
    expect(quiet.findAllComponents(IonToggle)).toHaveLength(0);
    expect(html(quiet)).toContain("Asks for nothing");
  });

  it("opens a tool, and asks once before removing it", async () => {
    const wrapper = sheet(IMAGE);
    const open = wrapper.find("[data-test='sheet-open']");
    expect(open.html()).toContain("Open");
    expect(open.findComponent(IonIcon).props("icon")).toBe(openOutline);
    await open.trigger("click");
    expect(wrapper.emitted("open")).toHaveLength(1);

    const remove = wrapper.find("[data-test='sheet-remove']");
    expect(remove.findComponent(IonButton).props("color")).toBe("danger");
    expect(remove.findComponent(IonIcon).props("icon")).toBe(trashOutline);
    await remove.trigger("click");
    expect(wrapper.emitted("remove")).toBeUndefined();
    expect(html(wrapper)).toContain("Remove it from this phone?");
    await wrapper.find("[data-test='remove-cancel']").trigger("click");
    expect(wrapper.find("[data-test='remove-confirm']").exists()).toBe(false);
    await wrapper.find("[data-test='sheet-remove']").trigger("click");
    await wrapper.find("[data-test='remove-confirm']").trigger("click");
    expect(wrapper.emitted("remove")).toHaveLength(1);
  });

  // A game is played, not opened; and removing it takes its saved games, so the sheet says so.
  it("plays a game, and warns that removing it deletes its saved games", async () => {
    const wrapper = sheet(CHESS);
    expect(wrapper.find("[data-test='sheet-open']").exists()).toBe(false);
    const playing = wrapper.find("[data-test='sheet-play']");
    expect(playing.html()).toContain("Play");
    expect(playing.findComponent(IonIcon).props("icon")).toBe(play);
    await playing.trigger("click");
    expect(wrapper.emitted("play")).toHaveLength(1);
    await wrapper.find("[data-test='sheet-remove']").trigger("click");
    expect(html(wrapper)).toContain("Removing it deletes its saved games on this phone.");
  });

  // Ioan, 2026-10-08: past the free days the tools are locked; the sheet offers the subscription.
  it("offers the subscription instead of opening a locked tool, never for a game", async () => {
    premiumLocked.value = true;
    const wrapper = sheet(IMAGE);
    const open = wrapper.find("[data-test='sheet-open']");
    expect(open.html()).toContain("Subscribe");
    expect(open.findComponent(IonIcon).props("icon")).toBe(lockClosedOutline);
    await open.trigger("click");
    expect(wrapper.emitted("open")).toHaveLength(1);
    expect(sheet(CHESS).find("[data-test='sheet-play']").exists()).toBe(true);
  });

  // Not installed yet: what it is and what it weighs, and a way to install it; no permissions.
  it("offers to install what is not on the phone", async () => {
    const notes = offer({ id: "com.flickertalk.notes", name: "Notes", summary: "Notes with a reminder.", size: 12_000, icon: "reader-outline" });
    const wrapper = sheet(notes);
    expect(wrapper.find("h2").text()).toBe("Notes");
    expect(wrapper.find("[data-test='sheet-meta']").text()).toBe("1.0.1 · 12 KB");
    expect(wrapper.find("[data-test='sheet-summary']").text()).toBe("Notes with a reminder.");
    expect(wrapper.findAllComponents(IonToggle)).toHaveLength(0);
    expect(html(wrapper)).not.toContain("Asks for nothing");
    expect(wrapper.find("[data-test='sheet-remove']").exists()).toBe(false);
    expect(wrapper.find("[data-test='sheet-open']").exists()).toBe(false);
    await wrapper.find("[data-test='sheet-install']").trigger("click");
    expect(wrapper.emitted("install")).toHaveLength(1);
    // The app carries it: added, nothing to download, no weight.
    const carried = sheet({ ...notes, carried: true });
    expect(carried.find("[data-test='sheet-meta']").text()).toBe("1.0.1");
  });

  it("does not install twice while installing", () => {
    const notes = offer({ id: "com.flickertalk.notes", name: "Notes" });
    const button = (busy: boolean) => sheet(notes, { busy }).findAllComponents(IonButton).find((one) => one.attributes("data-test") === "sheet-install")!;
    expect(button(true).props("disabled")).toBe(true);
    expect(button(false).props("disabled")).toBe(false);
  });

  // Dragged down, a tap outside, the back button: the page hears it; a question left open is
  // forgotten for the next time.
  it("says when Ionic dismisses it, and forgets the question about removing", async () => {
    const wrapper = sheet(IMAGE);
    await wrapper.find("[data-test='sheet-remove']").trigger("click");
    wrapper.findComponent(IonModalStub).vm.$emit("didDismiss");
    expect(wrapper.emitted("dismiss")).toHaveLength(1);
    await wrapper.setProps({ open: false, plugin: null });
    await wrapper.setProps({ open: true, plugin: QUIET });
    expect(wrapper.find("[data-test='remove-confirm']").exists()).toBe(false);
  });

  // 2026-10-08 ("Imagen por plugin"): the plugin's own image, in the 72 px box, when it has one.
  it("draws the plugin's own image when it has one", () => {
    const wrapper = sheet({ ...IMAGE, image: '<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>' });
    const box = wrapper.find(".ft-app-sheet__icon");
    expect(box.find("img").attributes("src")).toBe(pluginImage({ image: '<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>' }));
    expect(box.find("img").attributes("alt")).toBe("");
    expect(box.findComponent(IonIcon).exists()).toBe(false);
    expect(sheet(QUIET).find(".ft-app-sheet__icon img").exists()).toBe(false);
  });
});
