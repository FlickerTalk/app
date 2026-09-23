import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import AddContactPage from "./AddContactPage.vue";
import QrCode from "../components/QrCode.vue";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

const replace = vi.fn();
const query: Record<string, string> = {};
vi.mock("vue-router", () => ({ useRouter: () => ({ replace }), useRoute: () => ({ query }) }));
const scanner = vi.hoisted(() => ({ scan: vi.fn() }));
vi.mock("@tauri-apps/plugin-barcode-scanner", () => ({
  scan: scanner.scan,
  Format: { QRCode: "QR_CODE" },
}));

describe("AddContactPage", () => {
  beforeEach(() => {
    replace.mockClear();
    delete query.session;
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

  // The system share sheet (WhatsApp, Signal, mail…): nobody copies and pastes a code.
  it("shares the link through the phone's share sheet", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[aria-label='Share link']").trigger("click");
    await flushPromises();
    const shared = calls.find(([command]) => command === "core_share");
    expect(shared?.[1]).toEqual({ text: "Add me on FlickerTalk: https://flickertalk.com/add#card" });
  });

  // Where there is no share sheet (a desktop), the link is copied instead.
  it("copies the link where it cannot be shared", async () => {
    const writeText = vi.fn();
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    installTauri((command) => {
      if (command === "core_card") return "https://flickertalk.com/add#card";
      if (command === "core_share") throw new Error("not available on this platform");
      return undefined;
    });
    const wrapper = mount(AddContactPage, { shallow: true });
    await flushPromises();
    await wrapper.find("[aria-label='Share link']").trigger("click");
    await flushPromises();
    expect(writeText).toHaveBeenCalledWith("https://flickertalk.com/add#card");
    expect(wrapper.text()).toContain("Link copied");
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
    // Under a see-through app, which draws the way out (see scanner.ts).
    expect(scanner.scan).toHaveBeenCalledWith({ windowed: true, formats: ["QR_CODE"] });
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

  // Hidden sessions: opened from a session's QR button, the contact belongs to that session.
  it("adds the contact to the session it was opened from", async () => {
    query.session = "s1";
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    await wrapper.find("[data-test='paste']").setValue("https://flickertalk.com/add#theirs");
    await wrapper.find("[data-test='add']").trigger("click");
    await flushPromises();
    expect(calls).toContainEqual(["core_add_contact", { link: "https://flickertalk.com/add#theirs", session: "s1" }]);
  });

  // app#9: from a session, the QR is the session's own card, so whoever scans it lands there.
  it("shows the session's own card when opened from a session", async () => {
    query.session = "s1";
    mount(AddContactPage, { shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_card", { session: "s1" }]);
  });
});
