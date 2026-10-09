import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonIcon, IonItem, IonSegment, IonSegmentButton, IonTitle, IonToast, IonToggle } from "@ionic/vue";
import { constructOutline, extensionPuzzleOutline, gameControllerOutline, gridOutline, imageOutline } from "ionicons/icons";
import GamePermissions from "../components/GamePermissions.vue";
import AppsPage from "./AppsPage.vue";
import { pageShape } from "../__tests__/page-shape";
import AppTile from "../components/AppTile.vue";
import AppSheet from "../components/AppSheet.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { IonModalStub } from "../__tests__/ionic";
import { store } from "../core";
import { setLocale } from "../i18n";
import { installed as installedList, pluginImage, premiumLocked } from "../plugins";

const nav = vi.hoisted(() => ({ push: vi.fn(), route: { query: {} as Record<string, string> } }));
vi.mock("vue-router", async () => {
  const { reactive } = await import("vue");
  nav.route = reactive(nav.route);
  return { useRouter: () => ({ push: nav.push }), useRoute: () => nav.route };
});

const IMAGE = {
  id: "com.flickertalk.images",
  name: "Image",
  version: "1.0.1",
  icon: "image-outline",
  asks: { network: [], messages: true, send: "propose" },
  granted: { network: [], messages: false, send: "nothing" },
  installedAt: 1,
};
const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: false, send: "nothing" },
  granted: { network: [], messages: false, send: "nothing" },
  installedAt: 2,
};
const CHESS = {
  id: "com.flickertalk.game.chess",
  name: "Chess",
  version: "1.0.0",
  kind: "game",
  asks: { network: [], messages: false, send: "propose", live: true },
  granted: { network: [], messages: false, send: "nothing", live: false },
  installedAt: 3,
};
const READY = { ...CHESS, id: "com.flickertalk.game.connect4", name: "Connect 4", granted: { network: [], messages: false, send: "propose", live: true } };
const offer = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  id,
  name,
  version: "1.0.0",
  summary: `${name} does a thing.`,
  size: 48_000,
  installed: false,
  carried: false,
  ...extra,
});
const OFFERED = [
  offer(IMAGE.id, "Image", { installed: true, carried: true, size: 9_000, summary: "Shrinks and crops a picture." }),
  offer("com.flickertalk.sketch", "Sketch", { carried: true, size: 3_072 }),
  offer("com.flickertalk.notes", "Notes", { size: 12_000, icon: "reader-outline" }),
  offer(CHESS.id, "Chess", { installed: true, kind: "game" }),
  offer("com.flickertalk.game.go", "Go", { size: 1_200_000, kind: "game" }),
];

type Answer = (command: string, args?: Record<string, unknown>) => unknown;

/** The bridge: what is installed and offered; grants are kept, and anything else may be overridden. */
function answering({ installed = [IMAGE, CODE, CHESS, READY] as unknown[], offered = OFFERED as unknown[], also = (() => undefined) as Answer } = {}) {
  installTauri(async (command, args) => {
    calls.push([command, args]);
    const said = await also(command, args);
    if (said !== undefined) return said;
    if (command === "core_plugins") return installed;
    if (command === "core_catalogue") return offered;
    if (command === "core_plugin_grant") {
      installed = installed.map((one) => ((one as { id: string }).id === args?.plugin ? { ...(one as object), granted: args?.granted } : one));
    }
    if (command === "core_plugin_remove") installed = installed.filter((one) => (one as { id: string }).id !== args?.plugin);
    return undefined;
  });
}

const page = async () => {
  const wrapper = mount(AppsPage, { global: { stubs: { IonModal: IonModalStub } } });
  await flushPromises();
  return wrapper;
};
type Page = Awaited<ReturnType<typeof page>>;

const tile = (wrapper: Page, test: string) => wrapper.findAllComponents(AppTile).find((one) => one.attributes("data-test") === test);
const names = (wrapper: Page, selector: string) =>
  wrapper
    .findAllComponents(AppTile)
    .filter((one) => one.element.closest(selector))
    .map((one) => one.props("name") as string);
const showGames = async (wrapper: Page) => {
  wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
  await flushPromises();
};
const sheet = (wrapper: Page) => wrapper.findComponent(AppSheet);

describe("AppsPage", () => {
  beforeEach(() => {
    seed();
    nav.push.mockClear();
    nav.route.query = {};
    answering();
  });
  afterEach(() => {
    premiumLocked.value = false;
  });

  // 2026-10-08 (plan of the apps grid, screens 1 and 2): one tab for the tools and the games, apart.
  it("is the Apps tab, with the tools and the games in a segment", async () => {
    const wrapper = await page();
    expect(wrapper.findComponent(IonTitle).text()).toBe("Apps");
    const segment = wrapper.findComponent(IonSegment);
    expect(segment.props("value")).toBe("tools");
    const buttons = wrapper.findAllComponents(IonSegmentButton);
    expect(buttons.map((one) => one.props("value"))).toEqual(["tools", "games"]);
    expect(buttons.map((one) => one.findComponent(IonIcon).props("icon"))).toEqual([constructOutline, gameControllerOutline]);
    // Ionic's components are Stencil "scoped" elements: in happy-dom their text is only in the HTML.
    expect(buttons.map((one) => one.html())).toEqual([expect.stringContaining("Tools"), expect.stringContaining("Games")]);
  });

  it("shows the tools of this phone, then the tools it can add, dimmed, and never a game", async () => {
    const wrapper = await page();
    expect(names(wrapper, "[data-test='apps-installed']")).toEqual(["Code block", "Image"]);
    expect(tile(wrapper, `app-${IMAGE.id}`)!.props("icon")).toBe(imageOutline);
    expect(tile(wrapper, `app-${CODE.id}`)!.props("icon")).toBe(extensionPuzzleOutline);
    expect(wrapper.find("[data-test='apps-more-title']").text()).toBe("More tools");
    expect(names(wrapper, "[data-test='apps-more']")).toEqual(["Notes", "Sketch"]);
    const notes = tile(wrapper, "install-com.flickertalk.notes")!;
    expect(notes.props()).toMatchObject({ off: true, badge: "download", caption: "12 KB" });
    // The app carries it: added, nothing downloaded, nothing to weigh.
    const sketch = tile(wrapper, "install-com.flickertalk.sketch")!;
    expect(sketch.props()).toMatchObject({ off: true, badge: "add" });
    expect(sketch.props("caption")).toBeFalsy();
    expect(wrapper.text()).not.toContain("Chess");
    expect(wrapper.text()).not.toContain("Go");
  });

  it("shows the games on their segment, and says when every game is here already", async () => {
    answering({ offered: OFFERED.filter((one) => one.id !== "com.flickertalk.game.go") });
    const wrapper = await page();
    await showGames(wrapper);
    expect(names(wrapper, "[data-test='apps-installed']")).toEqual(["Chess", "Connect 4"]);
    expect(wrapper.find("[data-test='apps-more-title']").text()).toBe("More games");
    expect(wrapper.find("[data-test='apps-more']").exists()).toBe(false);
    expect(wrapper.find("[data-test='apps-all-here']").text()).toBe("All the games are on this phone");
    expect(tile(wrapper, `app-${CHESS.id}`)!.props("icon")).toBe(gameControllerOutline);
  });

  // The chat's apps sheet sends the user here for more games.
  it("opens on the games when asked to, also when the tab was already there", async () => {
    nav.route.query = { show: "games" };
    expect((await page()).findComponent(IonSegment).props("value")).toBe("games");

    nav.route.query = {};
    const wrapper = await page();
    expect(wrapper.findComponent(IonSegment).props("value")).toBe("tools");
    nav.route.query = { show: "games" };
    await flushPromises();
    expect(wrapper.findComponent(IonSegment).props("value")).toBe("games");
    // Settings' Tools row asks for the tools, even when the tab was left on the games.
    nav.route.query = { show: "tools" };
    await flushPromises();
    expect(wrapper.findComponent(IonSegment).props("value")).toBe("tools");
  });

  it("says when there is nothing yet, and when there is nothing to add offline", async () => {
    answering({ installed: [], offered: [] });
    const wrapper = await page();
    expect(wrapper.find("[data-test='apps-none']").text()).toBe("No tools yet");
    expect(wrapper.find("[data-test='apps-offline']").text()).toBe("Nothing to add right now. Check your connection and try again.");
    await showGames(wrapper);
    expect(wrapper.find("[data-test='apps-none']").text()).toBe("No games yet");
  });

  // A game installed from a chat's invitation shows when the tab comes back.
  it("reads the apps again each time the tab comes back", async () => {
    const wrapper = await page();
    const before = calls.filter(([command]) => command === "core_plugins").length;
    ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillEnter ?? []).forEach((hook) => hook());
    await flushPromises();
    expect(calls.filter(([command]) => command === "core_plugins").length).toBeGreaterThan(before);
  });

  // 2026-10-03 (updates): an update made in the background shows at once.
  it("follows the app's one list of what is installed", async () => {
    const wrapper = await page();
    installedList.value = [{ ...CODE, name: "Code" }] as never;
    await flushPromises();
    expect(names(wrapper, "[data-test='apps-installed']")).toEqual(["Code"]);
  });

  describe("a tap", () => {
    it("opens a tool on its own", async () => {
      const wrapper = await page();
      await tile(wrapper, `app-${IMAGE.id}`)!.trigger("click");
      expect(nav.push).toHaveBeenCalledWith(`/plugin/${IMAGE.id}`);
    });

    it("installs what is not here, with a spinner meanwhile, and grants it nothing", async () => {
      let finish = () => {};
      answering({ also: (command) => (command === "core_plugin_add" ? new Promise<void>((done) => (finish = () => done())).then(() => null) : undefined) });
      const wrapper = await page();
      await tile(wrapper, "install-com.flickertalk.notes")!.trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_plugin_add", { plugin: "com.flickertalk.notes" }]);
      expect(tile(wrapper, "install-com.flickertalk.notes")!.props("busy")).toBe(true);
      finish();
      await flushPromises();
      expect(tile(wrapper, "install-com.flickertalk.notes")?.props("busy")).toBeFalsy();
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_grant");
      expect(calls.filter(([command]) => command === "core_plugins").length).toBeGreaterThan(1);
    });

    // app#75: offline the tap seemed lost. A toast says so, floating: nothing in the grid moves.
    it("says in a toast when it cannot install", async () => {
      answering({ also: (command) => (command === "core_plugin_add" ? Promise.reject(new Error("download failed")) : undefined) });
      const wrapper = await page();
      const toast = () => wrapper.findComponent(IonToast);
      expect(toast().props("isOpen")).toBe(false);
      await tile(wrapper, "install-com.flickertalk.notes")!.trigger("click");
      await flushPromises();
      expect(toast().props("isOpen")).toBe(true);
      expect(toast().props("message")).toBe("It could not be installed. Check your connection and try again.");
      expect(toast().props("color")).toBe("danger");
      expect(tile(wrapper, "install-com.flickertalk.notes")!.props("busy")).toBeFalsy();
    });

    // Plan decision 11: a game needs the live channel and proposing, asked once; then who with.
    it("asks once for what a game needs, then who to play with, and opens the chat", async () => {
      const wrapper = await page();
      await showGames(wrapper);
      await tile(wrapper, `app-${CHESS.id}`)!.trigger("click");
      expect(wrapper.find("[data-test='game-permissions']").text()).toContain("Chess");
      // With the game's own icon (device review of app#121); Chess names none here.
      expect(wrapper.findComponent(GamePermissions).props("icon")).toBe(gameControllerOutline);
      await wrapper.find("[data-test='game-allow']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual([
        "core_plugin_grant",
        { plugin: CHESS.id, granted: { network: [], messages: false, send: "propose", live: true } },
      ]);
      const picker = wrapper.find("[data-test='contact-picker']");
      expect(picker.html()).toContain("Maria López");
      await wrapper.find("[data-test='play-with-c2']").trigger("click");
      expect(nav.push).toHaveBeenCalledWith(`/chat/c2?play=${CHESS.id}`);
    });

    // 2026-10-08 ("Imagen por plugin"): the tiles and the game's question draw a plugin's own image.
    it("draws a plugin's own image on its tile and on the game's question", async () => {
      const image = '<svg viewBox="0 0 64 64"><rect width="64" height="64"/></svg>';
      answering({ installed: [{ ...IMAGE, image }, CODE, { ...CHESS, image }], offered: [...OFFERED, offer("com.flickertalk.board", "Board", { image })] });
      const wrapper = await page();
      expect(tile(wrapper, `app-${IMAGE.id}`)!.props("image")).toBe(pluginImage({ image }));
      expect(tile(wrapper, `app-${CODE.id}`)!.props("image")).toBeUndefined();
      expect(tile(wrapper, "install-com.flickertalk.board")!.props("image")).toBe(pluginImage({ image }));
      await showGames(wrapper);
      await tile(wrapper, `app-${CHESS.id}`)!.trigger("click");
      expect(wrapper.findComponent(GamePermissions).props("image")).toBe(pluginImage({ image }));
    });

    it("asks with the icon the game names", async () => {
      answering({ installed: [{ ...CHESS, icon: "grid-outline" }] });
      const wrapper = await page();
      await showGames(wrapper);
      await tile(wrapper, `app-${CHESS.id}`)!.trigger("click");
      expect(wrapper.findComponent(GamePermissions).props("icon")).toBe(gridOutline);
    });

    it("plays nothing when the game's question is refused", async () => {
      const wrapper = await page();
      await showGames(wrapper);
      await tile(wrapper, `app-${CHESS.id}`)!.trigger("click");
      await wrapper.find("[data-test='game-cancel']").trigger("click");
      await flushPromises();
      expect(wrapper.find("[data-test='contact-picker']").exists()).toBe(false);
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_grant");
    });

    // §108: the contacts of an open hidden session too, never a blocked one.
    it("offers the contacts of an open hidden session, and none that is blocked", async () => {
      store.sessions = [{ id: "s1", chats: [{ ...store.chats[0], id: "h1", name: "Ana Hidden", messages: [] }], requests: [], circles: [] }];
      store.chats[1].blocked = true;
      const wrapper = await page();
      await showGames(wrapper);
      await tile(wrapper, `app-${READY.id}`)!.trigger("click");
      const picker = wrapper.find("[data-test='contact-picker']");
      expect(picker.html()).toContain("Ana Hidden");
      expect(picker.find("[data-test='play-with-c2']").exists()).toBe(false);
    });

    it("says so when there is nobody to play with", async () => {
      store.chats = [];
      const wrapper = await page();
      await showGames(wrapper);
      await tile(wrapper, `app-${READY.id}`)!.trigger("click");
      expect(wrapper.find("[data-test='contact-picker']").html()).toContain("Add a contact to play with");
      // Device review of app#121: it was text with nothing to tap. It leads to adding a contact.
      const add = wrapper.findAllComponents(IonItem).find((one) => one.attributes("data-test") === "picker-add-contact")!;
      // A button item (the bare attribute reaches Ionic as an empty string, which it takes as on).
      expect(add.props("button")).not.toBe(false);
      expect(add.props("button")).toBeDefined();
      await add.trigger("click");
      await flushPromises();
      expect(nav.push).toHaveBeenCalledWith("/add-contact");
      expect(wrapper.find("[data-test='contact-picker']").exists()).toBe(false);
    });
  });

  describe("a hold", () => {
    it("shows what an installed app is, and opens it from there", async () => {
      const wrapper = await page();
      expect(sheet(wrapper).props("open")).toBe(false);
      tile(wrapper, `app-${IMAGE.id}`)!.vm.$emit("hold");
      await flushPromises();
      expect(sheet(wrapper).props("open")).toBe(true);
      expect(sheet(wrapper).props("plugin")).toMatchObject({ id: IMAGE.id });
      expect(wrapper.find("[data-test='sheet-summary']").text()).toBe("Shrinks and crops a picture.");
      await wrapper.find("[data-test='sheet-open']").trigger("click");
      expect(nav.push).toHaveBeenCalledWith(`/plugin/${IMAGE.id}`);
      expect(sheet(wrapper).props("open")).toBe(false);
    });

    // §53: its permissions are switches; turned quickly one after the other, both are kept.
    it("grants a permission from the sheet, one change at a time", async () => {
      const wrapper = await page();
      tile(wrapper, `app-${IMAGE.id}`)!.vm.$emit("hold");
      await flushPromises();
      const [reads, writes] = wrapper.findAllComponents(IonToggle);
      reads.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
      writes.vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
      await vi.waitFor(() => expect(calls.filter(([command]) => command === "core_plugin_grant")).toHaveLength(2));
      await flushPromises();
      expect(calls.filter(([command]) => command === "core_plugin_grant").at(-1)?.[1]).toEqual({
        plugin: IMAGE.id,
        granted: { network: [], messages: true, send: "propose" },
      });
      expect(wrapper.findAllComponents(IonToggle).map((one) => one.props("checked"))).toEqual([true, true]);
    });

    it("puts a switch back when the core does not take the change", async () => {
      answering({ also: (command) => (command === "core_plugin_grant" ? Promise.reject(new Error("refused")) : undefined) });
      const wrapper = await page();
      tile(wrapper, `app-${IMAGE.id}`)!.vm.$emit("hold");
      await flushPromises();
      const field = { checked: true };
      const event = new CustomEvent("ionChange", { detail: { checked: true } });
      Object.defineProperty(event, "target", { value: field });
      wrapper.findAllComponents(IonToggle)[0].vm.$emit("ionChange", event);
      await vi.waitFor(() => expect(field.checked).toBe(false));
    });

    it("removes an app after asking, and closes", async () => {
      const wrapper = await page();
      tile(wrapper, `app-${IMAGE.id}`)!.vm.$emit("hold");
      await flushPromises();
      await wrapper.find("[data-test='sheet-remove']").trigger("click");
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_remove");
      await wrapper.find("[data-test='remove-confirm']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_plugin_remove", { plugin: IMAGE.id }]);
      expect(sheet(wrapper).props("open")).toBe(false);
      expect(names(wrapper, "[data-test='apps-installed']")).toEqual(["Code block"]);
    });

    it("plays a game from its sheet", async () => {
      const wrapper = await page();
      await showGames(wrapper);
      tile(wrapper, `app-${READY.id}`)!.vm.$emit("hold");
      await flushPromises();
      await wrapper.find("[data-test='sheet-play']").trigger("click");
      await flushPromises();
      expect(sheet(wrapper).props("open")).toBe(false);
      expect(wrapper.find("[data-test='contact-picker']").exists()).toBe(true);
    });

    it("shows what an app not installed yet is, and installs it from there", async () => {
      const wrapper = await page();
      tile(wrapper, "install-com.flickertalk.notes")!.vm.$emit("hold");
      await flushPromises();
      expect(sheet(wrapper).props("plugin")).toMatchObject({ id: "com.flickertalk.notes" });
      expect(wrapper.findAllComponents(IonToggle)).toHaveLength(0);
      await wrapper.find("[data-test='sheet-install']").trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_plugin_add", { plugin: "com.flickertalk.notes" }]);
      expect(sheet(wrapper).props("open")).toBe(false);
    });
  });

  // Ioan, 2026-10-08: past the free days, without the subscription, the tools are locked; a tap on
  // one goes to the Premium section of Settings. Games never are.
  describe("once the free days are over", () => {
    beforeEach(() => answering({ also: (command) => (command === "core_plan" ? { state: "limited", until: 0 } : undefined) }));

    it("locks the tools and sends a tap to the subscription, never a game", async () => {
      const wrapper = await page();
      expect(premiumLocked.value).toBe(true);
      const image = tile(wrapper, `app-${IMAGE.id}`)!;
      expect(image.props("badge")).toBe("lock");
      await image.trigger("click");
      expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");
      expect(nav.push).not.toHaveBeenCalledWith(`/plugin/${IMAGE.id}`);

      expect(tile(wrapper, "install-com.flickertalk.notes")!.props("badge")).toBe("lock");
      await tile(wrapper, "install-com.flickertalk.notes")!.trigger("click");
      await flushPromises();
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_add");

      await showGames(wrapper);
      expect(tile(wrapper, `app-${CHESS.id}`)!.props("badge")).toBeUndefined();
      expect(tile(wrapper, "install-com.flickertalk.game.go")!.props("badge")).toBe("download");
    });

    it("offers the subscription from a locked tool's sheet", async () => {
      const wrapper = await page();
      tile(wrapper, `app-${IMAGE.id}`)!.vm.$emit("hold");
      await flushPromises();
      expect(wrapper.find("[data-test='sheet-open']").html()).toContain("Subscribe");
      await wrapper.find("[data-test='sheet-open']").trigger("click");
      expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");
    });
  });

  // The plan may close while the tab is open: the core refuses, and the user goes where the
  // subscription is rather than being told the install failed.
  it("goes to the Premium section when the core refuses a tool", async () => {
    answering({ also: (command) => (command === "core_plugin_add" ? Promise.reject("needs_subscription") : undefined) });
    const wrapper = await page();
    await tile(wrapper, "install-com.flickertalk.notes")!.trigger("click");
    await flushPromises();
    expect(nav.push).toHaveBeenCalledWith("/tabs/settings#premium");
    expect(wrapper.findComponent(IonToast).props("isOpen")).toBe(false);
  });

  describe("on a Spanish phone", () => {
    afterEach(() => setLocale("en"));

    it("names the apps in Spanish, in the order a Spanish reader expects", async () => {
      await setLocale("es");
      answering({
        installed: [IMAGE, { ...CODE, locales: { es: { name: "Bloque de código" } } }],
        offered: [OFFERED[0], { ...OFFERED[1], locales: { es: { name: "Dibujo" } } }],
      });
      const wrapper = await page();
      expect(wrapper.findComponent(IonTitle).text()).toBe("Apps");
      expect(names(wrapper, "[data-test='apps-installed']")).toEqual(["Bloque de código", "Image"]);
      expect(names(wrapper, "[data-test='apps-more']")).toEqual(["Dibujo"]);
      expect(wrapper.find("[data-test='apps-more-title']").text()).toBe("Más herramientas");
    });
  });

  // Ionic's own shape (2026-10-09): the page's header and content are its own children, where
  // Ionic's transitions look for them, with nothing of ours in between.
  it("is an Ionic page: a header with the title and the segment, then the grid", () => {
    expect(pageShape(mount(AppsPage, { shallow: true }), ["app-sheet", "game-permissions"])).toEqual(["ion-header", "ion-content"]);
  });
});
