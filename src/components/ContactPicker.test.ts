import { beforeEach, describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonItem, IonModal, IonTitle } from "@ionic/vue";
import ContactPicker from "./ContactPicker.vue";
import { seed } from "../__tests__/seed";
import { IonModalStub } from "../__tests__/ionic";
import { store } from "../core";

// Plan 10.4 and Ioan, 2026-10-09: who a game is played with, or who a tool opened on its own sends
// to. The contacts of the main list and of each open hidden session (§108), never a blocked one;
// strangers who wrote first are not contacts yet.
function picker(props: Record<string, unknown> = {}) {
  return mount(ContactPicker, { props: { open: true, purpose: "play", ...props }, global: { stubs: { IonModal: IonModalStub } } });
}

describe("ContactPicker", () => {
  beforeEach(() => seed());

  it("is a sheet that lists the contacts and says which one was picked", async () => {
    const wrapper = picker();
    expect(wrapper.findComponent(IonModal).props("breakpoints")).toEqual([0, 0.5, 1]);
    expect(wrapper.findComponent(IonModal).props("initialBreakpoint")).toBe(0.5);
    expect(wrapper.findComponent(IonTitle).text()).toBe("Play with");
    expect(wrapper.find("[data-test='contact-picker']").html()).toContain("Maria López");
    await wrapper.find("[data-test='play-with-c2']").trigger("click");
    expect(wrapper.emitted("pick")).toEqual([["c2"]]);
  });

  it("is titled for what it picks for, and its rows say so", async () => {
    const wrapper = picker({ purpose: "send" });
    expect(wrapper.findComponent(IonTitle).text()).toBe("Send to");
    expect(wrapper.find("[data-test='play-with-c2']").exists()).toBe(false);
    await wrapper.find("[data-test='send-to-c2']").trigger("click");
    expect(wrapper.emitted("pick")).toEqual([["c2"]]);
  });

  it("offers the contacts of an open hidden session, and none that is blocked", () => {
    store.sessions = [{ id: "s1", chats: [{ ...store.chats[0], id: "h1", name: "Ana Hidden", messages: [] }], requests: [], circles: [] }];
    store.chats[1].blocked = true;
    const wrapper = picker();
    const list = wrapper.find("[data-test='contact-picker']");
    expect(list.html()).toContain("Ana Hidden");
    expect(list.find("[data-test='play-with-c2']").exists()).toBe(false);
    expect(list.find("[data-test='play-with-c1']").exists()).toBe(true);
  });

  it("leads to adding a contact when there is nobody", async () => {
    store.chats = [];
    const play = picker();
    expect(play.find("[data-test='contact-picker']").html()).toContain("Add a contact to play with");
    const send = picker({ purpose: "send" });
    expect(send.find("[data-test='contact-picker']").html()).toContain("Add a contact to send to");
    const add = send.findAllComponents(IonItem).find((one) => one.attributes("data-test") === "picker-add-contact")!;
    expect(add.props("button")).not.toBe(false);
    await add.trigger("click");
    expect(send.emitted("add")).toHaveLength(1);
  });

  it("says when it was dismissed", async () => {
    const wrapper = picker();
    wrapper.findComponent(IonModalStub).vm.$emit("didDismiss");
    await flushPromises();
    expect(wrapper.emitted("dismiss")).toHaveLength(1);
  });

  it("shows nothing while closed", () => {
    expect(picker({ open: false }).find("[data-test='contact-picker']").exists()).toBe(false);
  });
});
