import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonSelect } from "@ionic/vue";
import ContactPage from "./ContactPage.vue";
import { calls, seed } from "../__tests__/seed";

vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: "c1" } }), useRouter: () => ({ push: vi.fn() }) }));

describe("ContactPage", () => {
  beforeEach(() => seed());

  // Plan §29: the safety number both phones compute from the two identity keys.
  it("shows the contact and its security fingerprint", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    expect(wrapper.text()).toContain("Maria López");
    expect(calls).toContainEqual(["core_contact", { contact: "c1" }]);
    expect(wrapper.find("[data-test='fingerprint']").text()).toBe("a1b2 c3d4 e5f6 0718 293a 4b5c 6d7e 8f90 a1b2 c3d4 e5f6 0718");
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
});
