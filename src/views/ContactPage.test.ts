import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
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

  it("offers verifying in person, blocking and reporting", () => {
    const wrapper = mount(ContactPage, { shallow: true });
    for (const label of ["Verify in person", "Block", "Report"]) {
      expect(wrapper.text()).toContain(label);
    }
  });

  it("blocks the contact", async () => {
    const wrapper = mount(ContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[data-test='block']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_block", { contact: "c1", blocked: true }]);
  });
});
