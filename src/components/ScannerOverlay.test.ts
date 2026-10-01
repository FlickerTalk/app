import { beforeEach, describe, expect, it, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";

const scanning = vi.hoisted(() => ({ cancelScan: vi.fn(), openCameraSettings: vi.fn(), dismissRefusal: vi.fn() }));
vi.mock("../scanner", async () => {
  const { reactive } = await import("vue");
  const scanner = reactive({ active: false, refused: null as null | "denied" | "blocked" });
  scanning.dismissRefusal.mockImplementation(() => (scanner.refused = null));
  return { scanner, ...scanning };
});

// Android's back button: the handler the app listens with while something is open on top.
const back = vi.hoisted(() => ({ handler: null as null | (() => void) }));
vi.mock("@tauri-apps/api/app", () => ({
  onBackButtonPress: async (handler: () => void) => {
    back.handler = handler;
    return { unregister: async () => void (back.handler === handler && (back.handler = null)) };
  },
}));

import ScannerOverlay from "./ScannerOverlay.vue";
import { scanner } from "../scanner";

const settle = async () => {
  for (let at = 0; at < 5; at += 1) await Promise.resolve();
  await nextTick();
};

describe("ScannerOverlay", () => {
  beforeEach(() => {
    scanner.active = false;
    scanner.refused = null;
    back.handler = null;
    scanning.cancelScan.mockClear();
    scanning.openCameraSettings.mockClear();
  });

  it("only shows while the camera reads", () => {
    expect(mount(ScannerOverlay, { shallow: true }).find("[data-test='scanner-overlay']").exists()).toBe(false);
    scanner.active = true;
    expect(mount(ScannerOverlay, { shallow: true }).find("[data-test='scanner-overlay']").exists()).toBe(true);
  });

  // The way out the plugin's own view lacks.
  it("cancels the scan", async () => {
    scanner.active = true;
    await mount(ScannerOverlay, { shallow: true }).find("[aria-label='Cancel']").trigger("click");
    expect(scanning.cancelScan).toHaveBeenCalled();
  });

  // Android (2026-10-01): the camera draws under the WebView, which keeps the back button; with
  // nobody listening, Back left the page underneath and the camera stayed on.
  it("closes the camera on Back, and only that", async () => {
    mount(ScannerOverlay, { shallow: true });
    await settle();
    expect(back.handler).toBeNull();

    scanner.active = true;
    await settle();
    expect(back.handler).not.toBeNull();
    back.handler?.();
    expect(scanning.cancelScan).toHaveBeenCalledTimes(1);
  });

  describe("a refused camera", () => {
    it("says so, and points back to the link", async () => {
      scanner.refused = "denied";
      const wrapper = mount(ScannerOverlay, { shallow: true });
      const notice = wrapper.find("[data-test='camera-refused']");
      expect(notice.exists()).toBe(true);
      expect(notice.attributes("role")).toBe("alert");
      expect(notice.text()).toContain("Camera access is off");
      expect(notice.text()).toContain("paste");
      // The system will ask again on the next try: no detour through the settings.
      expect(wrapper.find("[data-test='camera-settings']").exists()).toBe(false);
    });

    it("refused for good, offers the system settings", async () => {
      scanner.refused = "blocked";
      const wrapper = mount(ScannerOverlay, { shallow: true });
      expect(wrapper.find("[data-test='camera-refused']").text()).toContain("Settings");
      await wrapper.find("[data-test='camera-settings']").trigger("click");
      expect(scanning.openCameraSettings).toHaveBeenCalled();
    });

    it("goes away with its close button or Back", async () => {
      scanner.refused = "denied";
      const wrapper = mount(ScannerOverlay, { shallow: true });
      await wrapper.find("[data-test='camera-refused'] [aria-label='Close']").trigger("click");
      expect(scanner.refused).toBeNull();
      await settle();
      expect(wrapper.find("[data-test='camera-refused']").exists()).toBe(false);

      scanner.refused = "blocked";
      await settle();
      back.handler?.();
      expect(scanner.refused).toBeNull();
    });
  });
});
