import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { IonAlert } from "@ionic/vue";
import AddContactPage from "./AddContactPage.vue";
import QrCode from "../components/QrCode.vue";
import { calls, fixture, seed } from "../__tests__/seed";
import { installTauri } from "../__tests__/tauri";

const replace = vi.fn();
const query: Record<string, string> = {};
vi.mock("vue-router", () => ({ useRouter: () => ({ replace }), useRoute: () => ({ query }) }));
const scanner = vi.hoisted(() => ({ scan: vi.fn() }));
// The camera was already allowed: asking for it is `scanner.test.ts`'s business.
vi.mock("@tauri-apps/plugin-barcode-scanner", () => ({
  scan: scanner.scan,
  checkPermissions: async () => "granted",
  requestPermissions: async () => "granted",
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

  // The big camera square is an icon and nothing else: it says what it does (§84).
  it("names the camera square for a screen reader", async () => {
    const wrapper = mount(AddContactPage, { shallow: true });
    await wrapper.find("[data-test='mode-scan']").trigger("click");
    expect(wrapper.find("[data-test='scanner']").attributes("aria-label")).toBe("Scan code");
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

  // app#78 (Samsung, 2026-10-03): adding the link of someone blocked kept them blocked and opened a
  // chat where nothing could be sent. Now it says they are blocked and offers to unblock them.
  describe("with the link of a contact who is blocked", () => {
    beforeEach(() => {
      installTauri((command, args) => {
        calls.push([command, args]);
        if (command === "core_card") return "https://flickertalk.com/add#card";
        if (command === "core_add_contact") return "ft_eve";
        if (command === "core_conversations") return [{ id: "ft_eve", name: "Eve", connected: false, unread: 0, blocked: true }];
        return undefined;
      });
    });

    async function addEve() {
      const wrapper = mount(AddContactPage, { shallow: true });
      await wrapper.find("[data-test='mode-scan']").trigger("click");
      await wrapper.find("[data-test='paste']").setValue("https://flickertalk.com/add#eve");
      await wrapper.find("[data-test='add']").trigger("click");
      await flushPromises();
      return wrapper;
    }

    type Button = { text: string; role?: string; handler?: () => unknown };
    const buttonsOf = (wrapper: Awaited<ReturnType<typeof addEve>>) => wrapper.findComponent(IonAlert).props("buttons") as Button[];

    it("says the contact is blocked instead of opening the chat", async () => {
      const wrapper = await addEve();
      const alert = wrapper.findComponent(IonAlert);
      expect(alert.props("isOpen")).toBe(true);
      expect(alert.props("header")).toBe("You blocked this contact");
      expect(replace).not.toHaveBeenCalled();
      expect(buttonsOf(wrapper).map((button) => button.text)).toEqual(["Cancel", "Unblock"]);
    });

    it("unblocks them and opens the chat when asked", async () => {
      const wrapper = await addEve();
      buttonsOf(wrapper).find((button) => button.text === "Unblock")!.handler!();
      await flushPromises();
      expect(calls).toContainEqual(["core_block", { contact: "ft_eve", blocked: false }]);
      expect(replace).toHaveBeenCalledWith("/chat/ft_eve");
    });

    it("leaves them blocked and stays here on cancel", async () => {
      const wrapper = await addEve();
      expect(buttonsOf(wrapper).find((button) => button.text === "Cancel")!.role).toBe("cancel");
      await wrapper.findComponent(IonAlert).vm.$emit("didDismiss");
      await flushPromises();
      expect(wrapper.findComponent(IonAlert).props("isOpen")).toBe(false);
      expect(calls.some(([command]) => command === "core_block")).toBe(false);
      expect(replace).not.toHaveBeenCalled();
    });
  });

  // app#9: from a session, the QR is the session's own card, so whoever scans it lands there.
  it("shows the session's own card when opened from a session", async () => {
    query.session = "s1";
    mount(AddContactPage, { shallow: true });
    await flushPromises();
    expect(calls).toContainEqual(["core_card", { session: "s1" }]);
  });
});
