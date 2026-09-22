import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import AddContactPage from "./AddContactPage.vue";
import QrCode from "../components/QrCode.vue";
import { calls, fixture, seed } from "../__tests__/seed";

const replace = vi.fn();
vi.mock("vue-router", () => ({ useRouter: () => ({ replace }) }));
const scanner = vi.hoisted(() => ({ scan: vi.fn() }));
vi.mock("@tauri-apps/plugin-barcode-scanner", () => ({
  scan: scanner.scan,
  Format: { QRCode: "QR_CODE" },
}));

describe("AddContactPage", () => {
  beforeEach(() => {
    replace.mockClear();
    seed();
  });

  // Plan §32: the QR code is this phone's signed Contact Card, as a link.
  it("shows my own card as a QR code so the other phone can scan it", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await flushPromises();
    expect(calls.map(([command]) => command)).toContain("core_card");
    expect(wrapper.findComponent(QrCode).props("value")).toBe("https://flickertalk.com/add#card");
    expect(wrapper.text()).toContain(fixture.me.id);
  });

  it("copies the link to share it another way", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const wrapper = mount(AddContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[aria-label='Share link']").trigger("click");
    expect(writeText).toHaveBeenCalledWith("https://flickertalk.com/add#card");
  });

  it("can switch to scanning the other code", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    expect(wrapper.find("[data-test='scanner']").exists()).toBe(true);
    expect(wrapper.findComponent(QrCode).exists()).toBe(false);
  });

  it("adds the contact whose code the camera reads", async () => {
    scanner.scan.mockResolvedValue({ content: "https://flickertalk.com/add#scanned", format: "QR_CODE" });
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    await wrapper.find("[data-test='scan-now']").trigger("click");
    await flushPromises();
    expect(scanner.scan).toHaveBeenCalledWith({ windowed: false, formats: ["QR_CODE"] });
    expect(calls).toContainEqual(["core_add_contact", { link: "https://flickertalk.com/add#scanned" }]);
  });

  // A link received through another app works like a scan.
  it("adds a contact from a pasted link and opens the conversation", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    await wrapper.find("[data-test='paste']").setValue("https://flickertalk.com/add#theirs");
    await wrapper.find("[data-test='add']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_add_contact", { link: "https://flickertalk.com/add#theirs" }]);
  });
});
