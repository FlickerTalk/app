import { describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import ContactPage from "./ContactPage.vue";

vi.mock("vue-router", () => ({ useRoute: () => ({ params: { id: "c1" } }), useRouter: () => ({ push: vi.fn() }) }));

describe("ContactPage", () => {
  it("shows the contact and its security fingerprint", () => {
    const wrapper = mount(ContactPage, { shallow: true });
    expect(wrapper.text()).toContain("Maria López");
    expect(wrapper.find("[data-test='fingerprint']").text().split(/\s+/)).toHaveLength(12);
  });

  it("offers verifying in person, blocking and reporting", () => {
    const wrapper = mount(ContactPage, { shallow: true });
    for (const label of ["Verify in person", "Block", "Report"]) {
      expect(wrapper.text()).toContain(label);
    }
  });
});
