import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import ChatsEmpty from "./ChatsEmpty.vue";
import { store } from "../core";
import { installTauri } from "../__tests__/tauri";

const push = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ push }) }));
const platforms = vi.hoisted(() => ({ on: [] as string[] }));
vi.mock("@ionic/vue", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@ionic/vue")>()),
  isPlatform: (name: string) => platforms.on.includes(name),
}));

const empty = (compact: boolean) => mount(ChatsEmpty, { props: { compact }, shallow: true });

describe("ChatsEmpty", () => {
  beforeEach(() => push.mockClear());
  afterEach(() => {
    platforms.on = [];
  });

  it("explains how to start and leads to adding a contact", async () => {
    const wrapper = empty(false);
    expect(wrapper.text()).toContain("No chats yet");
    await wrapper.find("button").trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });

  it("offers the quick tour as often as asked", async () => {
    const wrapper = empty(false);
    const link = wrapper.find("[data-test='tour-link']");
    expect(link.html()).toContain("How FlickerTalk works");
    await link.trigger("click");
    await link.trigger("click");
    expect(wrapper.emitted("tour")).toHaveLength(2);
    expect(push).not.toHaveBeenCalled();
  });

  it("in the strip, keeps only a named add-contact button", async () => {
    platforms.on = ["android"];
    const wrapper = empty(true);
    expect(wrapper.text()).toBe("");
    expect(wrapper.find("[data-test='tour-link']").exists()).toBe(false);
    expect(wrapper.find("[data-test='invite']").exists()).toBe(false);
    const buttons = wrapper.findAll("[data-test='empty-add']");
    expect(buttons).toHaveLength(1);
    expect(buttons[0].attributes("aria-label")).toBe("Add contact");
    await buttons[0].trigger("click");
    expect(push).toHaveBeenCalledWith("/add-contact");
  });

  it("on a phone, invites a friend from its contacts with my contact link", async () => {
    platforms.on = ["ios", "iphone"];
    store.me.name = "Ioan";
    const calls: Array<[string, unknown]> = [];
    installTauri((command, args) => {
      calls.push([command, args]);
      if (command === "core_card") return "https://flickertalk.com/add#card";
      if (command === "core_pick_contact") return { name: "Ana", whatsapp: "34612345678" };
      return undefined;
    });
    const wrapper = empty(false);
    const invite = wrapper.find("[data-test='invite']");
    expect(invite.attributes("aria-label")).toBe("Invite a friend");
    expect(invite.text()).toContain("Invite a friend");
    await invite.trigger("click");
    await flushPromises();
    expect(calls).toEqual([
      ["core_card", {}],
      ["core_pick_contact", {}],
      ["core_invite", { whatsapp: "34612345678", text: "Hi Ana, it's Ioan. I'm on FlickerTalk. Install it and tap here: https://flickertalk.com/add#card" }],
    ]);
    expect(push).not.toHaveBeenCalled();
  });

  it("does not offer invitations on a desktop", () => {
    platforms.on = ["desktop"];
    expect(empty(false).find("[data-test='invite']").exists()).toBe(false);
  });
});
