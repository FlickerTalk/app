import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonButton, IonItem, IonLabel, IonListHeader, IonToggle } from "@ionic/vue";
import GamesPage from "./GamesPage.vue";
import { calls, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";
import { store } from "../core";
import { IonModalStub } from "../__tests__/ionic";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));

const CODE = {
  id: "com.flickertalk.code",
  name: "Code block",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
};
const CHESS = {
  id: "com.flickertalk.game.chess",
  name: "Chess",
  version: "1.0.0",
  kind: "game",
  asks: { network: [], messages: false, send: "propose", live: true },
  granted: { network: [], messages: false, send: "nothing", live: false },
  installedAt: 2,
};
const READY = { ...CHESS, id: "com.flickertalk.game.connect4", name: "Connect 4", granted: { network: [], messages: false, send: "propose", live: true } };
const offer = (id: string, name: string, extra: Record<string, unknown> = {}) => ({
  id,
  name,
  version: "1.0.0",
  summary: `Play ${name}.`,
  size: 1_200_000,
  installed: false,
  carried: false,
  kind: "game",
  ...extra,
});
const OFFERED = [
  offer(CHESS.id, "Chess", { installed: true }),
  offer("com.flickertalk.game.go", "Go"),
  { ...offer("com.flickertalk.sketch", "Sketch"), kind: undefined },
];

/** The bridge for these tests: what is installed and offered, and what may fail. */
function answering({ installed = [CODE, CHESS, READY] as unknown[], offered = OFFERED as unknown[], addFails = false } = {}) {
  installTauri((command, args) => {
    calls.push([command, args]);
    if (command === "core_plugins") return installed;
    if (command === "core_catalogue") return offered;
    if (command === "core_plugin_add" && addFails) throw new Error("download failed");
    if (command === "core_plugin_grant") {
      installed = installed.map((one) => ((one as { id: string }).id === args?.plugin ? { ...(one as object), granted: args?.granted } : one));
    }
    return undefined;
  });
}

const page = async () => {
  // The sheets are the page's own components: rendered, not stubbed; Ionic's modal shows its
  // content only while open, as on the phone.
  const wrapper = mount(GamesPage, { shallow: true, global: { stubs: { GamePermissions: false, IonModal: IonModalStub } } });
  await flushPromises();
  return wrapper;
};

describe("GamesPage", () => {
  beforeEach(() => {
    seed();
    push.mockClear();
    answering();
  });

  // Plan 10.4: the games of this phone, and those the catalogue offers; never a tool.
  it("lists my games and the games I can add, and no tool", async () => {
    const wrapper = await page();
    const mine = wrapper.find("[data-test='my-games']").text();
    expect(mine).toContain("Chess");
    expect(mine).toContain("Connect 4");
    expect(mine).not.toContain("Code block");
    const more = wrapper.find("[data-test='more-games']").text();
    expect(more).toContain("Go");
    expect(more).toContain("Play Go.");
    expect(more).toContain("1.2 MB");
    expect(more).not.toContain("Sketch");
    // What is installed is not offered again.
    expect(wrapper.find("[data-test='install-com.flickertalk.game.chess']").exists()).toBe(false);
  });

  it("says when there is no game yet", async () => {
    answering({ installed: [CODE] });
    const wrapper = await page();
    expect(wrapper.text()).toContain("No games yet");
  });

  // The core answers offline with what the app carries, which has no game: say so honestly.
  it("says when there is no game to add", async () => {
    answering({ offered: [OFFERED[2]] });
    const wrapper = await page();
    expect(wrapper.find("[data-test='more-games']").exists()).toBe(false);
    expect(wrapper.text()).toContain("No games to add right now. Check your connection and try again.");
  });

  it("installs a game when asked, and grants it nothing", async () => {
    const wrapper = await page();
    await wrapper.find("[data-test='install-com.flickertalk.game.go']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_add", { plugin: "com.flickertalk.game.go" }]);
    expect(calls.map(([command]) => command)).not.toContain("core_plugin_grant");
    expect(calls.filter(([command]) => command === "core_plugins").length).toBeGreaterThan(1);
  });

  it("says so when a game cannot be installed", async () => {
    answering({ addFails: true });
    const wrapper = await page();
    await wrapper.find("[data-test='install-com.flickertalk.game.go']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[role='alert']").text()).toBe("The game could not be installed. Check your connection and try again.");
  });

  // Removing a game deletes its saved games: it warns before it does it.
  // Ioan, 2026-10-02: Ionic's components wherever one exists.
  it("uses Ionic's buttons for what a row does", async () => {
    const wrapper = await page();
    const buttons = () => wrapper.findAllComponents(IonButton).map((one) => one.attributes("data-test"));
    expect(buttons()).toEqual(expect.arrayContaining([`play-${CHESS.id}`, `remove-${CHESS.id}`, "install-com.flickertalk.game.go"]));
    await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
    expect(buttons()).toEqual(expect.arrayContaining(["remove-cancel", "remove-confirm"]));
  });

  it("removes a game only after warning that its saved games go with it", async () => {
    const wrapper = await page();
    await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
    expect(wrapper.text()).toContain("Removing it deletes its saved games on this phone.");
    expect(calls.map(([command]) => command)).not.toContain("core_plugin_remove");
    await wrapper.find("[data-test='remove-confirm']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_plugin_remove", { plugin: CHESS.id }]);
  });

  // As Settings asks before erasing: the question has a way back, and only one is asked at a time.
  describe("asking before removing", () => {
    const armed = (wrapper: Awaited<ReturnType<typeof page>>) => wrapper.findAll("[data-test='remove-confirm']");
    // Ionic keeps a tab mounted when another is in front: leaving it is what tells it.
    const leaveView = (wrapper: { vm: unknown }) =>
      ((wrapper.vm as unknown as Record<string, Array<() => void> | undefined>).onIonViewWillLeave ?? []).forEach((hook) => hook());

    it("goes back without removing anything when cancelled", async () => {
      const wrapper = await page();
      await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
      const cancel = wrapper.find("[data-test='remove-cancel']");
      expect(cancel.text()).toBe("Cancel");
      await cancel.trigger("click");
      expect(armed(wrapper)).toHaveLength(0);
      expect(wrapper.text()).not.toContain("Removing it deletes its saved games on this phone.");
      expect(wrapper.find(`[data-test='remove-${CHESS.id}']`).exists()).toBe(true);
      expect(calls.map(([command]) => command)).not.toContain("core_plugin_remove");
    });

    it("goes back when the trash is tapped again", async () => {
      const wrapper = await page();
      await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
      await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
      expect(armed(wrapper)).toHaveLength(0);
    });

    it("asks about one game at a time", async () => {
      const wrapper = await page();
      await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
      await wrapper.find(`[data-test='remove-${READY.id}']`).trigger("click");
      expect(armed(wrapper)).toHaveLength(1);
      await armed(wrapper)[0].trigger("click");
      await flushPromises();
      expect(calls).toContainEqual(["core_plugin_remove", { plugin: READY.id }]);
      expect(calls).not.toContainEqual(["core_plugin_remove", { plugin: CHESS.id }]);
    });

    it("forgets the question when the tab is left", async () => {
      const wrapper = await page();
      await wrapper.find(`[data-test='remove-${CHESS.id}']`).trigger("click");
      leaveView(wrapper);
      await flushPromises();
      expect(armed(wrapper)).toHaveLength(0);
    });
  });

  // Its permissions can be taken back here, as a tool's in Settings.
  it("shows a switch for each permission a game asks for", async () => {
    const wrapper = await page();
    const text = wrapper.find("[data-test='my-games']").text();
    expect(text).toContain("Talk to the same game on the other person's phone");
    expect(text).toContain("Write in the chat");
    const toggles = wrapper.findAllComponents(IonToggle);
    expect(toggles.length).toBe(4);
    toggles[0].vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    expect(calls.find(([command]) => command === "core_plugin_grant")?.[1]).toMatchObject({ plugin: CHESS.id, granted: { send: "propose" } });
  });

  // Plan decision 11: the first time, one sheet grants what a game needs; without it, no game.
  it("asks once for what a game needs before playing, and plays nothing if refused", async () => {
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${CHESS.id}']`).trigger("click");
    const sheet = wrapper.find("[data-test='game-permissions']");
    expect(sheet.text()).toContain("Chess");
    expect(sheet.text()).toContain("This game talks to the other person's phone and can leave the result in the chat.");

    await wrapper.find("[data-test='game-cancel']").trigger("click");
    await flushPromises();
    expect(wrapper.find("[data-test='game-permissions']").exists()).toBe(false);
    expect(wrapper.find("[data-test='contact-picker']").exists()).toBe(false);
    expect(calls.map(([command]) => command)).not.toContain("core_plugin_grant");
  });

  it("grants the live channel and proposing together, then asks who to play with", async () => {
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${CHESS.id}']`).trigger("click");
    await wrapper.find("[data-test='game-allow']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual([
      "core_plugin_grant",
      { plugin: CHESS.id, granted: { network: [], messages: false, send: "propose", live: true } },
    ]);
    expect(wrapper.find("[data-test='contact-picker']").exists()).toBe(true);
  });

  // Plan 10.4: the game opens in the conversation with the contact picked, with the live channel.
  it("plays a game that has what it needs with a contact of the list", async () => {
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${READY.id}']`).trigger("click");
    expect(wrapper.find("[data-test='game-permissions']").exists()).toBe(false);
    // Ionic's sheet modal with a name, and a row of Ionic's per contact.
    const sheet = wrapper.findAllComponents(IonModalStub).find((one) => one.attributes("aria-label") === "Play with");
    expect(sheet?.props("isOpen")).toBe(true);
    expect(wrapper.findAllComponents(IonItem).some((one) => one.attributes("data-test") === "play-with-c2")).toBe(true);
    const picker = wrapper.find("[data-test='contact-picker']");
    expect(picker.text()).toContain("Maria López");
    expect(picker.text()).toContain("Leo Martins");
    await wrapper.find("[data-test='play-with-c2']").trigger("click");
    expect(push).toHaveBeenCalledWith(`/chat/c2?play=${READY.id}`);
  });

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("offers contacts whose names read in their own direction", async () => {
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${READY.id}']`).trigger("click");
    const names = wrapper.find("[data-test='contact-picker']").findAllComponents(IonLabel);
    expect(names.length).toBeGreaterThan(1);
    for (const name of names) expect(name.attributes("dir")).toBe("auto");
  });

  // §108: the contacts of an open hidden session are there too; a blocked one is not.
  it("offers the contacts of an open hidden session, and none that is blocked", async () => {
    const hidden = { ...store.chats[0], id: "h1", name: "Ana Hidden", messages: [] };
    store.sessions = [{ id: "s1", chats: [hidden], requests: [], circles: [] }];
    store.chats[1].blocked = true;
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${READY.id}']`).trigger("click");
    const picker = wrapper.find("[data-test='contact-picker']");
    expect(picker.text()).toContain("Ana Hidden");
    // The session's contacts under a heading of their own.
    expect(wrapper.findComponent(IonListHeader).exists()).toBe(true);
    expect(picker.find("[data-test='play-with-c2']").exists()).toBe(false);
    await wrapper.find("[data-test='play-with-h1']").trigger("click");
    expect(push).toHaveBeenCalledWith(`/chat/h1?play=${READY.id}`);
  });

  it("says so when there is nobody to play with", async () => {
    store.chats = [];
    const wrapper = await page();
    await wrapper.find(`[data-test='play-${READY.id}']`).trigger("click");
    expect(wrapper.find("[data-test='contact-picker']").text()).toContain("Add a contact to play with");
  });
});
