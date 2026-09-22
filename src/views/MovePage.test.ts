import { beforeEach, describe, expect, it, vi } from "vitest";
import { flushPromises, mount } from "@vue/test-utils";
import { actions, move, resetMoving } from "../__tests__/moving-mock";
import MovePage from "./MovePage.vue";

const route = { query: { role: "new" } as Record<string, string> };
vi.mock("vue-router", () => ({ useRoute: () => route, useRouter: () => ({ back: vi.fn(), replace: vi.fn() }) }));
vi.mock("../moving", async () => (await import("../__tests__/moving-mock")).movingMock());
vi.mock("@tauri-apps/plugin-barcode-scanner", () => ({ scan: vi.fn(), Format: { QRCode: "QR_CODE" } }));

// §60: the new phone shows a QR; the old one scans it and hands everything over.
describe("MovePage", () => {
  beforeEach(() => resetMoving());

  it("shows the new phone's invite as a QR code", async () => {
    route.query = { role: "new" };
    const wrapper = mount(MovePage, { shallow: true });
    await flushPromises();
    expect(actions.invite).toHaveBeenCalled();
    expect(wrapper.findComponent({ name: "QrCode" }).props("value")).toBe("https://flickertalk.com/move#abc");
  });

  it("warns the old phone that it will be erased, then moves", async () => {
    route.query = { role: "old" };
    const wrapper = mount(MovePage, { shallow: true });
    expect(wrapper.text()).toContain("This phone will be erased");
    await wrapper.find("[data-test='move-link']").setValue("https://flickertalk.com/move#abc");
    await wrapper.find("[data-test='move-go']").trigger("click");
    expect(actions.moveTo).toHaveBeenCalledWith("https://flickertalk.com/move#abc");
  });

  it("shows the progress", () => {
    route.query = { role: "old" };
    Object.assign(move, { phase: "moving", done: 3, total: 12 });
    const wrapper = mount(MovePage, { shallow: true });
    expect(wrapper.find("[role='progressbar']").attributes("aria-valuenow")).toBe("25");
  });

  it("says when it is done, on each phone", () => {
    route.query = { role: "new" };
    move.phase = "received";
    expect(mount(MovePage, { shallow: true }).text()).toContain("Everything is here");
    route.query = { role: "old" };
    move.phase = "sent";
    expect(mount(MovePage, { shallow: true }).text()).toContain("Moved");
  });

  it("says why it failed", () => {
    route.query = { role: "old" };
    Object.assign(move, { phase: "failed", error: "the new phone cannot be reached directly" });
    expect(mount(MovePage, { shallow: true }).text()).toContain("the new phone cannot be reached directly");
  });
});
