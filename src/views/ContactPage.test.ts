import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonAlert, IonSelect, IonToggle } from "@ionic/vue";
import ContactPage from "./ContactPage.vue";
import { calls, seed } from "../__tests__/seed";
import { store } from "../core";

const routing = vi.hoisted(() => ({ id: "c1", push: vi.fn() }));
vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: routing.id } }), useRouter: () => ({ push: routing.push, replace: vi.fn() }) }));

describe("ContactPage", () => {
  beforeEach(() => {
    seed();
    routing.id = "c1";
    routing.push.mockClear();
  });

  // Ioan, 2026-10-06: as Messenger and WhatsApp do it, the chat header keeps few icons and the
  // contact page has a row of quick actions under the name: call, video and search.
  describe("quick actions", () => {
    const action = (wrapper: ReturnType<typeof mount>, name: string) => wrapper.find(`[data-test='quick-${name}']`);

    it("offers a call, a video call and a search, each with its label and an accessible name", () => {
      const wrapper = mount(ContactPage, { shallow: true });
      const row = wrapper.find("[data-test='quick-actions']");
      expect(row.exists()).toBe(true);
      expect(row.findAll("[data-test^='quick-']").map((one) => one.attributes("data-test"))).toEqual(["quick-call", "quick-video", "quick-search"]);
      expect([action(wrapper, "call"), action(wrapper, "video"), action(wrapper, "search")].map((one) => [one.text(), one.attributes("aria-label")])).toEqual([
        ["Call", "Voice call"],
        ["Video", "Video call"],
        ["Search", "Search in this conversation"],
      ]);
    });

    it("calls as the chat header does", async () => {
      const wrapper = mount(ContactPage, { shallow: true });
      await action(wrapper, "call").trigger("click");
      expect(routing.push).toHaveBeenCalledWith("/call/c1");
      await action(wrapper, "video").trigger("click");
      expect(routing.push).toHaveBeenCalledWith("/call/c1?video=1");
    });

    it("searches in the conversation", async () => {
      const wrapper = mount(ContactPage, { shallow: true });
      await action(wrapper, "search").trigger("click");
      expect(routing.push).toHaveBeenCalledWith("/chat/c1?search=1");
    });

    // A stranger who wrote first cannot be called yet, as in the chat header (A5).
    it("offers no call to someone whose request is still unanswered", () => {
      store.requests = [{ ...store.chats[1], id: "ft_stranger", name: "Mamá" }];
      routing.id = "ft_stranger";
      const wrapper = mount(ContactPage, { shallow: true });
      expect(action(wrapper, "call").exists()).toBe(false);
      expect(action(wrapper, "video").exists()).toBe(false);
      expect(action(wrapper, "search").exists()).toBe(true);
    });
  });

  // Plan §29: the safety number both phones compute from the two identity keys.
  it("shows the contact and its security fingerprint", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Maria López");
    expect(calls).toContainEqual(["core_contact", { contact: "c1" }]);
    expect(wrapper.find("[data-test='fingerprint']").text()).toBe("a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90 a1b2 c3d4 e5f6 0718");
  });

  // Seen in Arabic (2026-10-02): a name reads in its own direction, not in the app's.
  it("shows the name in its own direction", () => {
    expect(mount(ContactPage, { shallow: true }).find(".ft-contact__name").attributes("dir")).toBe("auto");
  });

  // The fingerprint card is how to verify in person; no separate button that does nothing.
  it("offers blocking and reporting", () => {
    const wrapper = mount(ContactPage, { shallow: true });
    expect(wrapper.text()).not.toContain("Verify in person");
    for (const label of ["Compare both codes", "Block", "Report"]) {
      expect(wrapper.text()).toContain(label);
    }
  });

  // §36: a reason, the messages as evidence only if asked, and the contact is blocked too.
  it("reports the contact by email and blocks them", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='report']").trigger("click");
    await wrapper.find("[data-test='reason-spam']").trigger("click");
    await wrapper.find("[data-test='send-report']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["plugin:opener|open_url", expect.objectContaining({ url: expect.stringContaining("Reason%3A%20spam") })]);
    expect(calls).toContainEqual(["core_block", { contact: "c1", blocked: true }]);
  });

  it("blocks the contact", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='block']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_block", { contact: "c1", blocked: true }]);
  });

  it("deletes the contact only after confirmation", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='delete-contact']").trigger("click");
    const alert = wrapper.findComponent(IonAlert);
    expect(alert.props("isOpen")).toBe(true);
    expect(calls).not.toContainEqual(["core_remove_contact", { contact: "c1" }]);

    const buttons = alert.props("buttons") as Array<{ text: string; handler?: () => void }>;
    await buttons.find((button) => button.text === "Delete")?.handler?.();
    await flushPromises();
    expect(calls).toContainEqual(["core_remove_contact", { contact: "c1" }]);
  });

  // Issue app#1: each contact carries their own name and history rules, kept on this phone.
  it("renames the contact", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    const name = wrapper.find("[data-test='name']");
    await name.setValue("Maria");
    await wrapper.find("[data-test='save-name']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_rename", { contact: "c1", name: "Maria" }]);
  });

  // app#77: once saved, the new name is what Save compares with, so going back is possible.
  it("lets the user go back to the previous name after saving a new one", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    const name = wrapper.find("[data-test='name']");
    const save = () => wrapper.find<HTMLButtonElement>("[data-test='save-name']").element;
    const loaded = (name.element as HTMLInputElement).value;
    await name.setValue("Maria");
    save().click();
    await flushPromises();
    expect(save().disabled).toBe(true);
    await name.setValue(loaded);
    expect(save().disabled).toBe(false);
  });

  it("sets how long the history lasts and when read messages burn", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    const [history, burn] = wrapper.findAllComponents(IonSelect);
    history.vm.$emit("ionChange", { detail: { value: 7 * 86_400 } });
    await flushPromises();
    expect(calls).toContainEqual(["core_set_history", { contact: "c1", keepFor: 604800, burnAfterRead: 0 }]);
    burn.vm.$emit("ionChange", { detail: { value: 300 } });
    await flushPromises();
    expect(calls).toContainEqual(["core_set_history", { contact: "c1", keepFor: 604800, burnAfterRead: 300 }]);
  });

  // Issues app#4–#6: mute, what is accepted from them and whether they get receipts. Each switch
  // saves at once, the others kept.
  it("changes what this phone takes from the contact", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    const toggle = (name: string) =>
      wrapper.findAllComponents(IonToggle).find((one) => one.attributes("data-test") === name)!;
    for (const name of ["mute", "chat", "calls", "receipts", "typing"]) expect(toggle(name).exists()).toBe(true);
    expect(toggle("mute").attributes("checked")).toBe("false");
    expect(toggle("calls").attributes("checked")).toBe("true");

    toggle("mute").vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: true } }));
    await flushPromises();
    expect(calls).toContainEqual(["core_set_rules", { contact: "c1", rules: { muted: true, acceptsChat: true, acceptsCalls: true, receipts: true, typing: true } }]);
    toggle("calls").vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: false } }));
    await flushPromises();
    expect(calls).toContainEqual(["core_set_rules", { contact: "c1", rules: { muted: true, acceptsChat: true, acceptsCalls: false, receipts: true, typing: true } }]);
    // 2026-10-05: whether they see "typing…" is a rule like the others.
    expect(toggle("typing").attributes("checked")).toBe("true");
    toggle("typing").vm.$emit("ionChange", new CustomEvent("ionChange", { detail: { checked: false } }));
    await flushPromises();
    expect(calls).toContainEqual(["core_set_rules", { contact: "c1", rules: { muted: true, acceptsChat: true, acceptsCalls: false, receipts: true, typing: false } }]);
  });
});
