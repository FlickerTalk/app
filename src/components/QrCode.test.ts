import { describe, expect, it } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import QrCode from "./QrCode.vue";

describe("QrCode", () => {
  it("draws the code for the given value", async () => {
    const wrapper = mount(QrCode, { props: { value: "ft_74MxNcJ8E2kQ" } });
    await flushPromises();
    expect(wrapper.html()).toContain("<svg");
  });

  it("is announced as a QR code", async () => {
    const wrapper = mount(QrCode, { props: { value: "ft_74MxNcJ8E2kQ" } });
    await flushPromises();
    expect(wrapper.find("[role='img']").attributes("aria-label")).toBe("QR code");
  });
});
