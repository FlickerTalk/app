import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonSegment, IonSegmentButton } from "@ionic/vue";
import { installTauri } from "../__tests__/tauri";
import { premiumLocked, refreshPlugins } from "../plugins";
import AppTile from "./AppTile.vue";
import ChatApps from "./ChatApps.vue";

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
  granted: { network: [], messages: false, send: "propose", live: true },
  installedAt: 2,
};

async function installed(plugins: object[]) {
  installTauri((command) => (command === "core_plugins" ? plugins : undefined));
  await refreshPlugins();
}

const apps = (selected: "tools" | "games") => mount(ChatApps, { props: { selected }, global: { stubs: { IonIcon: true } } });
const tiles = (wrapper: ReturnType<typeof apps>) => wrapper.findAllComponents(AppTile);

describe("ChatApps", () => {
  beforeEach(() => installed([CODE, CHESS]));
  afterEach(() => (premiumLocked.value = false));

  it("has a segment for the tools and one for the games, and hands up the one chosen", () => {
    const wrapper = apps("tools");
    expect(wrapper.findComponent(IonSegment).props("value")).toBe("tools");
    expect(wrapper.findAllComponents(IonSegmentButton).map((one) => [one.props("value"), one.find("ion-label").element.innerHTML])).toEqual([
      ["tools", "Tools"],
      ["games", "Games"],
    ]);
    wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "games" } });
    wrapper.findComponent(IonSegment).vm.$emit("ionChange", { detail: { value: "anything" } });
    expect(wrapper.emitted("update:selected")).toEqual([["games"], ["tools"]]);
  });

  it("shows the tools in a grid, a lock on each while they are locked, and opens one", async () => {
    premiumLocked.value = true;
    const wrapper = apps("tools");
    expect(wrapper.find("[data-test='apps-sheet-tools']").classes()).toContain("ft-app-grid");
    expect(tiles(wrapper).map((one) => [one.attributes("data-test"), one.props("name"), one.props("badge")])).toEqual([[`app-${CODE.id}`, "Code block", "lock"]]);
    await tiles(wrapper)[0].trigger("click");
    expect(wrapper.emitted("use")).toEqual([[CODE.id]]);
  });

  it("shows the games, plays one on a tap and invites on a hold, and leads to more", async () => {
    const wrapper = apps("games");
    const chess = tiles(wrapper)[0];
    expect(chess.attributes("data-test")).toBe(`game-${CHESS.id}`);
    expect(chess.attributes("aria-label")).toBe("Chess. Touch and hold to invite");
    await chess.trigger("click");
    chess.vm.$emit("hold");
    expect(wrapper.emitted("play")).toEqual([[CHESS.id]]);
    expect(wrapper.emitted("invite")?.[0][0]).toMatchObject({ id: CHESS.id });
    await wrapper.find("[data-test='more-games-link']").trigger("click");
    expect(wrapper.emitted("more")).toHaveLength(1);
  });

  it("says when there is no tool or no game yet, and still leads to more games", async () => {
    await installed([]);
    expect(apps("tools").find(".ft-apps-sheet__note").text()).toBe("No tools yet");
    const games = apps("games");
    expect(games.find("[data-test='games-sheet']").text()).toBe("No games yet");
    expect(games.find("[data-test='more-games-link']").exists()).toBe(true);
  });
});
