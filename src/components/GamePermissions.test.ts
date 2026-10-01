import { describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import GamePermissions from "./GamePermissions.vue";

describe("GamePermissions", () => {
  // Plan decision 11: one sheet says what a game does with the other phone and the chat.
  it("says what the game will do and lets the user allow it or not", async () => {
    const wrapper = mount(GamePermissions, { props: { name: "Chess" }, shallow: true });
    expect(wrapper.find("[role='dialog']").attributes("aria-label")).toBe("Chess");
    expect(wrapper.text()).toContain("Chess");
    expect(wrapper.text()).toContain("This game talks to the other person's phone and can leave the result in the chat.");
    expect(wrapper.find("[data-test='game-allow']").text()).toBe("Allow and play");

    await wrapper.find("[data-test='game-allow']").trigger("click");
    await wrapper.find("[data-test='game-cancel']").trigger("click");
    expect(wrapper.emitted("allow")).toHaveLength(1);
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });

  // A game that is not here yet is a download: the sheet says what it weighs and installs it.
  it("says what a game that is not installed weighs, and installs it to play", () => {
    const wrapper = mount(GamePermissions, { props: { name: "Chess", size: 1_200_000 }, shallow: true });
    expect(wrapper.text()).toContain("1.2 MB");
    expect(wrapper.find("[data-test='game-allow']").text()).toBe("Install and play");
  });

  it("goes away when the user taps outside it", async () => {
    const wrapper = mount(GamePermissions, { props: { name: "Chess" }, shallow: true });
    await wrapper.find("[data-test='game-permissions']").trigger("click");
    expect(wrapper.emitted("cancel")).toHaveLength(1);
  });
});
