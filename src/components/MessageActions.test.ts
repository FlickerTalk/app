import { afterEach, describe, expect, it } from "vitest";
import { mount } from "@vue/test-utils";
import { IonButton } from "@ionic/vue";
import type { Chat, ChatMessage, PluginView } from "../core";
import { premiumLocked } from "../plugins";
import MessageActions from "./MessageActions.vue";
import source from "./MessageActions.vue?raw";

const THEIRS: ChatMessage = { id: "m1", mine: false, text: "Are you coming tonight?", time: "09:00", sentAt: 1 };
const MINE: ChatMessage = { id: "m2", mine: true, text: "Yes", time: "09:01", sentAt: 1 };
const ALEX = { id: "c2", name: "Alex Chen" } as Chat;
const NOTES = {
  id: "com.flickertalk.notes",
  name: "Notes",
  version: "1.0.0",
  asks: { network: [], messages: true, send: "nothing" },
  granted: { network: [], messages: true, send: "nothing" },
  installedAt: 1,
} as PluginView;

const sheet = (props: Record<string, unknown> = {}) =>
  mount(MessageActions, {
    props: {
      message: THEIRS,
      folded: false,
      canEdit: false,
      request: false,
      forwarding: false,
      openingWith: false,
      erasing: false,
      others: [ALEX],
      openers: [],
      viewable: false,
      ...props,
    },
    global: { stubs: { IonIcon: true } },
  });

const ionButton = (wrapper: ReturnType<typeof sheet>, test: string) => wrapper.findAllComponents(IonButton).find((one) => one.attributes("data-test") === test);

describe("MessageActions", () => {
  afterEach(() => (premiumLocked.value = false));

  it("offers six reactions, the chosen one lit, and no reactions to a stranger", async () => {
    const wrapper = sheet({ message: { ...THEIRS, reactions: { mine: "❤️" } } });
    const reactions = wrapper.findAll(".ft-actions__react");
    expect(reactions.map((one) => one.text())).toEqual(["👍", "❤️", "😂", "😮", "😢", "🙏"]);
    expect(reactions[1].classes()).toContain("is-active");
    expect(reactions[1].attributes("aria-label")).toBe("Remove reaction");
    expect(reactions[1].attributes("aria-pressed")).toBe("true");
    await reactions[0].trigger("click");
    expect(wrapper.emitted("react")).toEqual([["👍"]]);
    expect(sheet({ request: true }).find(".ft-actions__emoji").exists()).toBe(false);
  });

  it("asks for each thing to do with the message", async () => {
    const wrapper = sheet({ canEdit: true, openers: [NOTES] });
    for (const [test, event] of [
      ["edit", "edit"],
      ["pin", "pin"],
      ["reply", "reply"],
      ["fold", "fold"],
      ["share", "share"],
    ] as const) {
      await wrapper.find(`[data-test='${test}']`).trigger("click");
      expect(wrapper.emitted(event), test).toHaveLength(1);
    }
    await wrapper.find("[data-test='forward']").trigger("click");
    await wrapper.find("[data-test='open-with']").trigger("click");
    await wrapper.find("[data-test='delete']").trigger("click");
    expect(wrapper.emitted("update:forwarding")).toEqual([[true]]);
    expect(wrapper.emitted("update:openingWith")).toEqual([[true]]);
    expect(wrapper.emitted("update:erasing")).toEqual([[true]]);
  });

  it("uses Ionic's clear buttons for edit, pin and answer, the pin lit once pinned", () => {
    const wrapper = sheet({ canEdit: true, message: { ...MINE, pinned: true } });
    for (const test of ["edit", "pin", "reply"]) expect(ionButton(wrapper, test)?.props("fill"), test).toBe("clear");
    expect(ionButton(wrapper, "pin")?.props("color")).toBe("primary");
    expect(ionButton(wrapper, "pin")?.attributes("aria-label")).toBe("Unpin");
    expect(sheet().find("[data-test='edit']").exists()).toBe(false);
    expect(sheet({ request: true }).find("[data-test='reply']").exists()).toBe(false);
    expect(sheet().find("[data-test='open-with']").exists()).toBe(false);
  });

  it("names folding by what it would do", () => {
    expect(sheet().find("[data-test='fold']").attributes("aria-label")).toBe("Fold");
    expect(sheet({ folded: true }).find("[data-test='fold']").attributes("aria-label")).toBe("Unfold");
  });

  it("deletes here after asking, and for both sides only my message not written for later", async () => {
    const theirs = sheet({ erasing: true });
    expect(theirs.find("[data-test='delete']").exists()).toBe(false);
    await theirs.find("[data-test='delete-sure']").trigger("click");
    expect(theirs.emitted("erase")).toHaveLength(1);
    expect(theirs.find("[data-test='delete-everyone']").exists()).toBe(false);

    const mine = sheet({ erasing: true, message: MINE });
    await mine.find("[data-test='delete-everyone']").trigger("click");
    expect(mine.emitted("eraseEverywhere")).toHaveLength(1);
    expect(sheet({ erasing: true, message: { ...MINE, scheduledFor: 1 } }).find("[data-test='delete-everyone']").exists()).toBe(false);
    expect(sheet({ erasing: true, message: { ...MINE, deleted: true } }).find("[data-test='delete-everyone']").exists()).toBe(false);
  });

  it("forwards to a contact whose name reads in its own direction", async () => {
    const wrapper = sheet({ forwarding: true });
    const to = wrapper.find("[data-test='to-c2']");
    expect(to.text()).toBe("Alex Chen");
    expect(to.attributes("dir")).toBe("auto");
    await to.trigger("click");
    expect(wrapper.emitted("forwardTo")).toEqual([["c2"]]);
    expect(sheet({ forwarding: true, others: [] }).find(".ft-actions__bar").text()).toContain("—");
  });

  it("opens with a tool, a lock on a locked one, and another app when a viewer takes the tap", async () => {
    premiumLocked.value = true;
    const wrapper = sheet({ openingWith: true, openers: [NOTES], viewable: true });
    const notes = wrapper.find(`[data-test='open-with-${NOTES.id}']`);
    expect(notes.text()).toBe("Notes");
    expect(notes.find("[aria-label='Subscribe to use the tools']").exists()).toBe(true);
    await notes.trigger("click");
    expect(wrapper.emitted("openWith")).toEqual([[NOTES.id]]);
    await wrapper.find("[data-test='open-elsewhere']").trigger("click");
    expect(wrapper.emitted("openElsewhere")).toHaveLength(1);
    expect(sheet({ openingWith: true, openers: [NOTES] }).find("[data-test='open-elsewhere']").exists()).toBe(false);
  });

  it("closes when the dim backdrop is tapped, not the bar", async () => {
    const wrapper = sheet();
    await wrapper.find(".ft-actions__bar").trigger("click");
    expect(wrapper.emitted("close")).toBeUndefined();
    await wrapper.find("[data-test='actions']").trigger("click");
    expect(wrapper.emitted("close")).toHaveLength(1);
  });

  it("dims and centres on the conversation's own box, not on the Ionic page around it", () => {
    const styles = source.slice(source.indexOf("<style"));
    const layer = /\.ft-actions\s*\{([^}]*)\}/.exec(styles)?.[1] ?? "";
    expect(layer).toMatch(/position:\s*absolute;/);
    expect(layer).toMatch(/inset:\s*0;/);
  });
});
