import { beforeEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";

import EmojiPicker from "./EmojiPicker.vue";
import { GROUPS, recent, remember } from "../emoji";

describe("EmojiPicker", () => {
  beforeEach(() => localStorage.clear());

  it("opens on the first group and shows its emoji", () => {
    const wrapper = mount(EmojiPicker, { shallow: true });
    const shown = wrapper.findAll("[data-test='emoji']").map((one) => one.text());
    expect(shown).toEqual(GROUPS[0].emoji);
  });

  it("changes group when its tab is pressed", async () => {
    const wrapper = mount(EmojiPicker, { shallow: true });
    await wrapper.find(`[data-test='group-${GROUPS[2].id}']`).trigger("click");
    expect(wrapper.findAll("[data-test='emoji']").map((one) => one.text())).toEqual(GROUPS[2].emoji);
  });

  it("gives back the emoji that was pressed, and remembers it", async () => {
    const wrapper = mount(EmojiPicker, { shallow: true });
    await wrapper.findAll("[data-test='emoji']")[3].trigger("click");
    expect(wrapper.emitted("pick")).toEqual([[GROUPS[0].emoji[3]]]);
    expect(recent()).toEqual([GROUPS[0].emoji[3]]);
  });

  // What was used lately is where the thumb expects it: first tab, and the one that opens.
  it("opens on what was used lately when there is some", () => {
    remember("🎉");
    const wrapper = mount(EmojiPicker, { shallow: true });
    expect(wrapper.findAll("[data-test='emoji']").map((one) => one.text())).toEqual(["🎉"]);
    expect(wrapper.find("[data-test='group-recent']").exists()).toBe(true);
  });

  it("has no recent tab when nothing was used yet", () => {
    const wrapper = mount(EmojiPicker, { shallow: true });
    expect(wrapper.find("[data-test='group-recent']").exists()).toBe(false);
  });

  it("names every emoji and every tab for a screen reader", () => {
    const wrapper = mount(EmojiPicker, { shallow: true });
    expect(wrapper.find(`[data-test='group-${GROUPS[0].id}']`).attributes("aria-label")).toBeTruthy();
    expect(wrapper.findAll("[data-test='emoji']")[0].attributes("aria-label")).toBe(GROUPS[0].emoji[0]);
  });
});
